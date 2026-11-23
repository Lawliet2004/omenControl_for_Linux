//! Read-only client telemetry supplement for a device-isolated daemon. Never
//! used by the privileged service. Mock/failed responses are never supplemented.
use omen_core::capability::{Capabilities, Capability};
use omen_core::sensors::{SensorSample, SensorSource};
use omen_core::{OmenError, Result};
use omen_hardware::sensors::nvidia;
use omen_ipc::protocol::DaemonInfo;
use omen_ipc::{IpcClient, Op, Response};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct MonitoringClient {
    ipc: IpcClient,
    local: Mutex<LocalCache>,
}

#[derive(Default)]
struct LocalCache {
    gpu: Option<String>,
    updated: Option<Instant>,
    samples: Vec<SensorSample>,
}

impl LocalCache {
    fn read(
        &mut self,
        gpu: Option<String>,
        sample: impl FnOnce() -> Vec<SensorSample>,
    ) -> Vec<SensorSample> {
        if gpu.is_none() {
            *self = Self::default();
            return Vec::new();
        }
        if self.gpu != gpu
            || self
                .updated
                .is_none_or(|time| time.elapsed() >= Duration::from_secs(5))
        {
            self.samples = sample();
            self.updated = Some(Instant::now());
            self.gpu = gpu;
        }
        self.samples.clone()
    }
}

impl MonitoringClient {
    pub fn new(ipc: IpcClient) -> Self {
        Self {
            ipc,
            local: Mutex::new(LocalCache::default()),
        }
    }

    pub fn ipc(&self) -> &IpcClient {
        &self.ipc
    }

    pub fn call(&self, op: Op) -> Result<Response> {
        let response = self.ipc.call(op.clone())?;
        if !response.ok
            || !matches!(
                op,
                Op::GetSensors | Op::GetCapabilities | Op::ExportDiagnostics
            )
        {
            return Ok(response);
        }
        // SAFETY: geteuid has no preconditions or mutable pointer arguments.
        let unprivileged = unsafe { libc::geteuid() } != 0;
        let real = unprivileged
            && (self.ipc.peer_key().is_none() || self.ipc.root_owned_peer())
            && self
                .ipc
                .call_decode::<DaemonInfo>(Op::GetInfo)
                .is_ok_and(|info| !info.mock);
        supplement(op, response, real, || {
            let gpu = nvidia::primary_gpu(&omen_hardware::SysRoot::default())
                .filter(|gpu| gpu.runtime_status.as_deref() != Some("suspended"))
                .map(|gpu| gpu.pci_address);
            self.local
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .read(gpu, nvidia::read_nvidia_samples)
        })
    }
}

fn supplement(
    op: Op,
    mut response: Response,
    real_unprivileged: bool,
    sample: impl FnOnce() -> Vec<SensorSample>,
) -> Result<Response> {
    if !real_unprivileged || !response.ok {
        return Ok(response);
    }
    match op {
        Op::GetSensors => {
            let mut samples: Vec<SensorSample> = response.decode()?;
            if needs_supplement(&samples) {
                samples.extend(sample());
                response.data = Some(json(&samples)?);
            }
        }
        Op::GetCapabilities => {
            let mut caps: Capabilities = response.decode()?;
            if !caps.nvidia_telemetry.is_available() && !sample().is_empty() {
                caps.nvidia_telemetry = local_capability();
                response.data = Some(json(&caps)?);
            }
        }
        Op::ExportDiagnostics => {
            let mut value: serde_json::Value = response.decode()?;
            // Require an explicit real report, even if a daemon changed between
            // the report and GetInfo calls. Fixture measurements stay fixtures.
            if value.get("mock").and_then(|v| v.as_bool()) != Some(false) {
                return Ok(response);
            }
            let mut samples: Vec<SensorSample> =
                serde_json::from_value(value["sensors"].clone())
                    .map_err(|e| OmenError::Internal(format!("diagnostic sensors: {e}")))?;
            if needs_supplement(&samples) {
                let local = sample();
                if !local.is_empty() {
                    let ids: Vec<_> = local.iter().map(|s| s.id.clone()).collect();
                    samples.extend(local);
                    value["sensors"] = json(&samples)?;
                    value["capabilities"]["nvidia_telemetry"] = json(&local_capability())?;
                    value["client_telemetry"] = serde_json::json!({
                        "nvidia": {"origin": "local_client", "mode": "read_only", "sensor_ids": ids}
                    });
                    response.data = Some(value);
                }
            }
        }
        _ => {}
    }
    Ok(response)
}

fn needs_supplement(samples: &[SensorSample]) -> bool {
    !samples
        .iter()
        .any(|s| matches!(s.source, SensorSource::NvidiaSmi | SensorSource::Mock))
}

fn local_capability() -> Capability {
    Capability::available("read-only nvidia-smi samples from the local client; privileged daemon keeps its device sandbox")
}

fn json(value: &impl serde::Serialize) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|e| OmenError::Internal(format!("local telemetry: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use omen_core::sensors::SensorKind;

    fn gpu() -> SensorSample {
        SensorSample::numeric(
            "nvidia/temp",
            "GPU temperature (NVIDIA)",
            SensorKind::Temperature,
            SensorSource::NvidiaSmi,
            47.0,
        )
    }

    #[test]
    fn cache_preserves_timestamps_and_clears_on_suspend_or_adapter_change() {
        let mut cache = LocalCache::default();
        let first = cache.read(Some("pci0".into()), || vec![gpu()]);
        assert_eq!(cache.read(Some("pci0".into()), || panic!("cached")), first);
        assert!(cache
            .read(None, || panic!("must not wake suspended GPU"))
            .is_empty());
        assert_eq!(cache.read(Some("pci0".into()), || vec![gpu()]).len(), 1);
        assert!(cache.read(Some("pci1".into()), Vec::new).is_empty());
        assert!(cache
            .read(Some("pci1".into()), || panic!("failures are cached too"))
            .is_empty());
        cache.updated = Some(Instant::now() - Duration::from_secs(6));
        assert_eq!(cache.read(Some("pci1".into()), || vec![gpu()]).len(), 1);
    }

    #[test]
    fn mock_root_failed_or_existing_nvidia_responses_never_sample_locally() {
        for real in [false, true] {
            let response = Response::ok(17, vec![gpu()]);
            assert_eq!(
                supplement(Op::GetSensors, response.clone(), real, || panic!(
                    "daemon NVIDIA is authoritative"
                ))
                .unwrap()
                .data,
                response.data
            );
        }
        let response = Response::ok(2, Vec::<SensorSample>::new());
        assert!(
            supplement(Op::GetSensors, response, false, || panic!("mock or root"))
                .unwrap()
                .ok
        );
        let mut fixture = gpu();
        fixture.source = SensorSource::Mock;
        assert!(
            supplement(
                Op::GetSensors,
                Response::ok(1, vec![fixture]),
                true,
                || panic!("fixture sample")
            )
            .unwrap()
            .ok
        );
        let response = Response::err(1, OmenError::DaemonUnavailable("gone".into()));
        assert!(
            !supplement(Op::GetSensors, response, true, || panic!("failed read"))
                .unwrap()
                .ok
        );
    }

    #[test]
    fn supplement_preserves_backend_values_response_id_and_local_measurement_time() {
        let cpu = SensorSample::numeric(
            "cpu/temp",
            "CPU",
            SensorKind::Temperature,
            SensorSource::Hwmon,
            55.0,
        );
        let local = gpu();
        let response = supplement(
            Op::GetSensors,
            Response::ok(42, vec![cpu.clone()]),
            true,
            || vec![local.clone()],
        )
        .unwrap();
        assert_eq!(response.id, 42);
        assert_eq!(
            response.decode::<Vec<SensorSample>>().unwrap(),
            vec![cpu, local]
        );
    }

    #[test]
    fn diagnostics_require_real_identity_and_record_local_origin_and_ids() {
        for mock in [serde_json::json!(true), serde_json::Value::Null] {
            let response = Response::ok(1, serde_json::json!({"mock":mock,"sensors":[]}));
            assert!(
                supplement(Op::ExportDiagnostics, response, true, || panic!(
                    "not explicitly real"
                ))
                .unwrap()
                .ok
            );
        }
        let response = Response::ok(
            1,
            serde_json::json!({"mock":false,"sensors":[],"capabilities":{}}),
        );
        let response = supplement(Op::ExportDiagnostics, response, true, || vec![gpu()]).unwrap();
        let value: serde_json::Value = response.decode().unwrap();
        assert_eq!(
            value["client_telemetry"]["nvidia"]["origin"],
            "local_client"
        );
        assert_eq!(
            value["client_telemetry"]["nvidia"]["sensor_ids"],
            serde_json::json!(["nvidia/temp"])
        );
        assert_eq!(value["sensors"][0]["value"], 47.0);
    }

    #[test]
    fn capability_supplement_changes_only_missing_nvidia_telemetry() {
        use omen_hardware::HardwareBackend;
        let backend =
            omen_hardware::mock::MockBackend::new(omen_hardware::mock::MockModel::VictusFb);
        let mut caps = backend.capabilities();
        caps.nvidia_telemetry = Capability::unavailable("device sandbox");
        let original = caps.clone();
        let response = supplement(
            Op::GetCapabilities,
            Response::ok(1, caps.clone()),
            true,
            || vec![gpu()],
        )
        .unwrap();
        let mut changed: Capabilities = response.decode().unwrap();
        assert_eq!(changed.nvidia_telemetry, local_capability());
        changed.nvidia_telemetry = original.nvidia_telemetry;
        assert_eq!(changed, caps);
        caps.nvidia_telemetry = Capability::available("daemon NVML");
        let response = supplement(
            Op::GetCapabilities,
            Response::ok(1, caps.clone()),
            true,
            || panic!("daemon already available"),
        )
        .unwrap();
        assert_eq!(response.decode::<Capabilities>().unwrap(), caps);
    }
}
