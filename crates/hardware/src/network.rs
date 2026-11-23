//! Batched /proc/net/dev counters plus read-only sysfs link identity/state.
//! No addresses, packet contents, sockets, subprocesses or policy writes.
use crate::{fsutil, SysRoot};
use omen_core::network::{NetworkInterface, NetworkSnapshot, NetworkSource};
use omen_core::{OmenError, Result};
use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1_048_576;
const MAX_INTERFACES: usize = 128;
const CACHE_INTERVAL: Duration = Duration::from_millis(250);
const MAX_RATE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    ifindex: u32,
    inode: u64,
}

#[derive(Clone)]
struct Counters {
    identity: Identity,
    rx: Option<u64>,
    tx: Option<u64>,
}

#[derive(Default)]
pub struct NetworkReader {
    sampled: Option<Duration>,
    previous: BTreeMap<String, Counters>,
    cached: Option<NetworkSnapshot>,
}

fn identity(path: &Path) -> Option<Identity> {
    let ifindex = u32::try_from(fsutil::read_u64(&path.join("ifindex"))?)
        .ok()
        .filter(|v| *v > 0)?;
    Some(Identity {
        ifindex,
        inode: path.metadata().ok()?.ino(),
    })
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 16
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '/' | ':' | '\0'))
}

fn rate(old: Option<u64>, new: Option<u64>, seconds: Option<f64>) -> Option<f64> {
    let (old, new, seconds) = (old?, new?, seconds?);
    new.checked_sub(old)
        .map(|delta| delta as f64 / seconds)
        .filter(|v| v.is_finite())
}

impl NetworkReader {
    pub fn snapshot(&mut self, root: &SysRoot) -> Result<NetworkSnapshot> {
        let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
        // SAFETY: time points to writable timespec storage; CLOCK_BOOTTIME is
        // monotonic and includes Linux suspend time, unlike CLOCK_MONOTONIC.
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, time.as_mut_ptr()) } != 0 {
            return Err(OmenError::Io(format!(
                "network sampling clock: {}",
                std::io::Error::last_os_error()
            )));
        }
        let time = unsafe { time.assume_init() };
        if time.tv_sec < 0 || !(0..1_000_000_000).contains(&time.tv_nsec) {
            return Err(OmenError::Io(
                "network sampling clock returned an invalid time".into(),
            ));
        }
        self.snapshot_at(root, Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
    }

    fn snapshot_at(&mut self, root: &SysRoot, now: Duration) -> Result<NetworkSnapshot> {
        if self
            .sampled
            .and_then(|t| now.checked_sub(t))
            .is_some_and(|d| d < CACHE_INTERVAL)
        {
            if let Some(cached) = &self.cached {
                return Ok(cached.clone());
            }
        }
        let result = self.acquire(root, now);
        if result.is_err() {
            self.sampled = None;
            self.previous.clear();
            self.cached = None;
        }
        result
    }

    fn acquire(&mut self, root: &SysRoot, now: Duration) -> Result<NetworkSnapshot> {
        let class = root.join("sys/class/net");
        let entries = std::fs::read_dir(&class)
            .map_err(|e| OmenError::Unsupported(format!("network interfaces unreadable: {e}")))?;
        // Capture identity before and after the non-atomic procfs/sysfs reads.
        let before: BTreeMap<String, Identity> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_str()?.to_string();
                valid_name(&name)
                    .then(|| identity(&entry.path()).map(|id| (name, id)))
                    .flatten()
            })
            .collect();
        let mut text = String::new();
        std::fs::File::open(root.join("proc/net/dev"))
            .and_then(|file| file.take(MAX_BYTES + 1).read_to_string(&mut text))
            .map_err(|e| OmenError::Unsupported(format!("network counters unreadable: {e}")))?;
        if text.len() as u64 > MAX_BYTES {
            return Err(OmenError::Unsupported(
                "network counter report exceeds 1 MiB".into(),
            ));
        }
        let mut lines = text.lines();
        if !lines.next().is_some_and(|s| {
            s.contains("Inter-") && s.contains("Receive") && s.contains("Transmit")
        }) || !lines.next().is_some_and(|s| s.contains("face"))
        {
            return Err(OmenError::Unsupported(
                "network counter report has an unrecognized header".into(),
            ));
        }
        let elapsed = self
            .sampled
            .and_then(|t| now.checked_sub(t))
            .filter(|d| !d.is_zero() && *d <= MAX_RATE_INTERVAL);
        let seconds = elapsed.map(|d| d.as_secs_f64());
        let mut report = NetworkSnapshot {
            source: if root.path() == Path::new("/") {
                NetworkSource::Kernel
            } else {
                NetworkSource::Fixture
            },
            measured_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            interval_ms: elapsed.map(|d| d.as_millis() as u64),
            interfaces: Vec::new(),
            omitted_interfaces: 0,
            unreadable_interfaces: 0,
        };
        let mut rows = BTreeMap::new();
        for line in lines.filter(|line| !line.trim().is_empty()) {
            let Some((name, values)) = line.rsplit_once(':') else {
                report.unreadable_interfaces += 1;
                continue;
            };
            let name = name.trim();
            if !valid_name(name) || rows.contains_key(name) {
                report.unreadable_interfaces += 1;
                continue;
            }
            let values: Vec<_> = values.split_whitespace().collect();
            if values.len() < 16 {
                report.unreadable_interfaces += 1;
                continue;
            }
            rows.insert(name.to_string(), values);
        }
        let mut previous = BTreeMap::new();
        for (name, values) in rows {
            if report.interfaces.len() >= MAX_INTERFACES {
                report.omitted_interfaces += 1;
                continue;
            }
            let path = class.join(&name);
            let Some(id) = identity(&path).filter(|id| before.get(&name) == Some(id)) else {
                report.unreadable_interfaces += 1;
                continue;
            };
            let counter = |index: usize| values[index].parse::<u64>().ok();
            let (rx, tx) = (counter(0), counter(8));
            let old = self.previous.get(&name).filter(|old| old.identity == id);
            let state = fsutil::read_trim(&path.join("operstate")).filter(|s| {
                matches!(
                    s.as_str(),
                    "unknown"
                        | "notpresent"
                        | "down"
                        | "lowerlayerdown"
                        | "testing"
                        | "dormant"
                        | "up"
                )
            });
            let flags = fsutil::read_trim(&path.join("flags"))
                .and_then(|value| u32::from_str_radix(value.trim_start_matches("0x"), 16).ok());
            report.interfaces.push(NetworkInterface {
                name: name.clone(),
                ifindex: id.ifindex,
                link_state: state,
                loopback: flags.map(|flags| flags & 0x8 != 0),
                rx_bytes: rx,
                tx_bytes: tx,
                rx_errors: counter(2),
                tx_errors: counter(10),
                rx_dropped_or_missed: counter(3),
                tx_dropped: counter(11),
                rx_bytes_per_second: rate(old.and_then(|v| v.rx), rx, seconds),
                tx_bytes_per_second: rate(old.and_then(|v| v.tx), tx, seconds),
            });
            previous.insert(
                name,
                Counters {
                    identity: id,
                    rx,
                    tx,
                },
            );
        }
        self.previous = previous;
        self.sampled = Some(now);
        self.cached = Some(report.clone());
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn interface(root: &SysRoot, name: &str, index: u32) {
        let path = root.join("sys/class/net").join(name);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("ifindex"), index.to_string()).unwrap();
        std::fs::write(path.join("operstate"), "up").unwrap();
        std::fs::write(path.join("flags"), "0x1003").unwrap();
    }
    fn counters(root: &SysRoot, rows: &str) {
        std::fs::create_dir_all(root.join("proc/net")).unwrap();
        std::fs::write(root.join("proc/net/dev"),format!("Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n{rows}")).unwrap();
    }
    fn row(name: &str, rx: &str, tx: u64) -> String {
        format!("{name}: {rx} 1 2 3 0 0 0 0 {tx} 1 4 5 0 0 0 0\n")
    }
    #[test]
    fn deltas_cache_and_reset_preserve_real_acquisition_times() {
        let temp = tempfile::tempdir().unwrap();
        let root = SysRoot::new(temp.path());
        interface(&root, "wlan0", 3);
        counters(&root, &row("wlan0", "1000", 2000));
        let mut reader = NetworkReader::default();
        let start = Duration::from_secs(100);
        let first = reader.snapshot_at(&root, start).unwrap();
        assert_eq!(first.source, NetworkSource::Fixture);
        assert_eq!(first.interval_ms, None);
        assert_eq!(first.interfaces[0].rx_bytes_per_second, None);
        assert_eq!(first.interfaces[0].rx_dropped_or_missed, Some(3));
        counters(&root, &row("wlan0", "1512", 4048));
        assert_eq!(
            reader
                .snapshot_at(&root, start + Duration::from_millis(100))
                .unwrap(),
            first
        );
        let next = reader
            .snapshot_at(&root, start + Duration::from_secs(2))
            .unwrap();
        assert_eq!(next.interval_ms, Some(2000));
        assert_eq!(next.interfaces[0].rx_bytes_per_second, Some(256.0));
        assert_eq!(next.interfaces[0].tx_bytes_per_second, Some(1024.0));
        counters(&root, &row("wlan0", "1", 4048));
        let reset = reader
            .snapshot_at(&root, start + Duration::from_secs(3))
            .unwrap();
        assert_eq!(reset.interfaces[0].rx_bytes_per_second, None);
        assert_eq!(reset.interfaces[0].tx_bytes_per_second, Some(0.0));
        let gap = reader
            .snapshot_at(&root, start + Duration::from_secs(40))
            .unwrap();
        assert_eq!(gap.interval_ms, None);
        assert_eq!(gap.interfaces[0].tx_bytes_per_second, None);
    }
    #[test]
    fn missing_invalid_replaced_and_disappeared_interfaces_rebase() {
        let temp = tempfile::tempdir().unwrap();
        let root = SysRoot::new(temp.path());
        interface(&root, "eth0", 2);
        let start = Duration::from_secs(100);
        let mut reader = NetworkReader::default();
        counters(&root, &row("eth0", "100", 200));
        reader.snapshot_at(&root, start).unwrap();
        interface(&root, "eth0", 4);
        counters(&root, &row("eth0", "1000000", 2000000));
        assert_eq!(
            reader
                .snapshot_at(&root, start + Duration::from_secs(1))
                .unwrap()
                .interfaces[0]
                .rx_bytes_per_second,
            None
        );
        counters(&root, &row("eth0", "bad", 2000000));
        let invalid = reader
            .snapshot_at(&root, start + Duration::from_secs(2))
            .unwrap();
        assert_eq!(invalid.interfaces[0].rx_bytes, None);
        counters(&root, &row("eth0", "1000001", 2000000));
        assert_eq!(
            reader
                .snapshot_at(&root, start + Duration::from_secs(3))
                .unwrap()
                .interfaces[0]
                .rx_bytes_per_second,
            None
        );
        counters(&root, "");
        assert!(reader
            .snapshot_at(&root, start + Duration::from_secs(4))
            .unwrap()
            .interfaces
            .is_empty());
        counters(&root, &row("eth0", "1000002", 2000000));
        assert_eq!(
            reader
                .snapshot_at(&root, start + Duration::from_secs(5))
                .unwrap()
                .interfaces[0]
                .rx_bytes_per_second,
            None
        );
        std::fs::remove_file(root.join("proc/net/dev")).unwrap();
        assert!(reader
            .snapshot_at(&root, start + Duration::from_secs(6))
            .is_err());
        counters(&root, &row("eth0", "1000003", 2000000));
        assert_eq!(
            reader
                .snapshot_at(&root, start + Duration::from_secs(7))
                .unwrap()
                .interval_ms,
            None
        );
    }
    #[test]
    fn malformed_rows_limits_and_private_fields_are_handled_explicitly() {
        let temp = tempfile::tempdir().unwrap();
        let root = SysRoot::new(temp.path());
        let mut rows = String::from("broken\n../escape: 1 2\n");
        for i in 0..130 {
            let name = format!("veth{i:03}");
            interface(&root, &name, i + 1);
            rows += &row(&name, "1", 2);
        }
        counters(&root, &rows);
        let snapshot = NetworkReader::default().snapshot(&root).unwrap();
        assert_eq!(snapshot.interfaces.len(), 128);
        assert_eq!(snapshot.omitted_interfaces, 2);
        assert_eq!(snapshot.unreadable_interfaces, 2);
        let json = serde_json::to_value(&snapshot).unwrap();
        assert!(json["interfaces"][0].get("address").is_none());
        std::fs::write(
            root.join("proc/net/dev"),
            "x".repeat(MAX_BYTES as usize + 1),
        )
        .unwrap();
        assert!(NetworkReader::default().snapshot(&root).is_err());
        std::fs::write(root.join("proc/net/dev"), "bad header").unwrap();
        assert!(NetworkReader::default().snapshot(&root).is_err());
    }

    #[test]
    fn recreated_identity_loopback_and_unavailable_metadata_are_not_guessed() {
        let temp = tempfile::tempdir().unwrap();
        let root = SysRoot::new(temp.path());
        interface(&root, "lo", 1);
        std::fs::write(root.join("sys/class/net/lo/flags"), "0x9").unwrap();
        counters(&root, &row("lo", "100", 100));
        let mut reader = NetworkReader::default();
        let start = Duration::from_secs(100);
        assert_eq!(
            reader.snapshot_at(&root, start).unwrap().interfaces[0].loopback,
            Some(true)
        );
        std::fs::rename(
            root.join("sys/class/net/lo"),
            root.join("sys/class/net/old-lo"),
        )
        .unwrap();
        interface(&root, "lo", 1);
        counters(&root, &row("lo", "100000", 100000));
        std::fs::write(root.join("sys/class/net/lo/operstate"), "not-a-state").unwrap();
        std::fs::remove_file(root.join("sys/class/net/lo/flags")).unwrap();
        let replacement = reader
            .snapshot_at(&root, start + Duration::from_secs(1))
            .unwrap();
        assert_eq!(replacement.interfaces[0].rx_bytes_per_second, None);
        assert_eq!(replacement.interfaces[0].loopback, None);
        assert_eq!(replacement.interfaces[0].link_state, None);
        std::fs::remove_file(root.join("sys/class/net/lo/ifindex")).unwrap();
        let gone = reader
            .snapshot_at(&root, start + Duration::from_secs(2))
            .unwrap();
        assert!(gone.interfaces.is_empty());
        assert_eq!(gone.unreadable_interfaces, 1);
    }
}
