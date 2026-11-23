//! Bounded subprocess helpers for trusted system utilities. No shell parsing.
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub fn output(command: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut data = Vec::new();
        let result = stdout.take(131_073).read_to_end(&mut data).map(|_| data);
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(result.err().unwrap_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "system utility timed out")
                }));
            }
        }
    };
    let stdout = rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "utility output timed out")
        })??;
    if stdout.len() > 131_072 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "utility output exceeds 128 KiB",
        ));
    }
    Ok(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kills_and_reaps_timed_out_utility() {
        let start = Instant::now();
        let e = output(Command::new("sleep").arg("3"), Duration::from_millis(50)).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
