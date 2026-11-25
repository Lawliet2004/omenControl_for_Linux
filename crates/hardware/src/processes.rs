//! Lightweight process enumeration and resource sampling for System Vitals.
//! Zero external dependencies; parses `/proc/{pid}/stat` safely without panicking.

use omen_core::sensors::ProcessSample;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn page_size_bytes() -> u64 {
    // SAFETY: sysconf(_SC_PAGESIZE) is thread-safe and has no pointer arguments.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size > 0 {
        size as u64
    } else {
        4096
    }
}

struct ParsedStat {
    pid: u32,
    name: String,
    ticks: u64,
    starttime: u64,
    rss_bytes: u64,
}

/// Parse a single `/proc/{pid}/stat` content into process identity, CPU ticks, and RSS.
fn parse_proc_stat_full(content: &str, page_bytes: u64) -> Option<ParsedStat> {
    let open_paren = content.find('(')?;
    let close_paren = content.rfind(')')?;
    if close_paren <= open_paren {
        return None;
    }
    let pid: u32 = content[..open_paren].trim().parse().ok()?;
    let name = content[open_paren + 1..close_paren].to_string();
    let rest = content[close_paren + 1..].trim();
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After ')', fields start from state (field 3 of /proc/pid/stat).
    // utime is field 14 -> index 11
    // stime is field 15 -> index 12
    // starttime is field 22 -> index 19
    // rss is field 24 -> index 21 (pages)
    if fields.len() < 22 {
        return None;
    }
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    let starttime: u64 = fields.get(19)?.parse().ok()?;
    let rss_pages: u64 = fields.get(21)?.parse().ok()?;
    let rss_bytes = rss_pages.saturating_mul(page_bytes);
    Some(ParsedStat {
        pid,
        name,
        ticks: utime.saturating_add(stime),
        starttime,
        rss_bytes,
    })
}

#[cfg(test)]
fn parse_proc_stat(content: &str) -> Option<(u32, String, u64, u64)> {
    let parsed = parse_proc_stat_full(content, page_size_bytes())?;
    Some((parsed.pid, parsed.name, parsed.ticks, parsed.rss_bytes))
}

/// Stateful `/proc` process sampler that computes delta CPU utilization across polls.
#[derive(Default)]
pub struct ProcessMonitor {
    prev_total_ticks: Option<u64>,
    prev_procs: HashMap<u32, (u64, u64)>,
}

impl ProcessMonitor {
    pub fn reset(&mut self) {
        self.prev_total_ticks = None;
        self.prev_procs.clear();
    }

    /// Scan `proc_root`, compute CPU % relative to total system CPU ticks since the
    /// previous call, exclude zero-RSS kernel threads, and return top `limit` by RSS.
    pub fn sample(&mut self, proc_root: &Path, limit: usize) -> Vec<ProcessSample> {
        let Ok(entries) = fs::read_dir(proc_root) else {
            return Vec::new();
        };
        let page_bytes = page_size_bytes();
        let total_ticks =
            crate::sensors::cpu_ticks(&proc_root.join("stat")).map(|(total, _)| total);
        let total_delta = match (self.prev_total_ticks, total_ticks) {
            (Some(prev), Some(curr)) => curr.checked_sub(prev).filter(|&d| d > 0),
            _ => None,
        };

        let mut next_procs = HashMap::new();
        let mut samples = Vec::new();
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            if !name_str.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let stat_path = entry.path().join("stat");
            let Ok(content) = fs::read_to_string(&stat_path) else {
                continue;
            };
            let Some(parsed) = parse_proc_stat_full(&content, page_bytes) else {
                continue;
            };
            if parsed.rss_bytes == 0 {
                continue;
            }
            next_procs.insert(parsed.pid, (parsed.starttime, parsed.ticks));
            let cpu_pct = match (total_delta, self.prev_procs.get(&parsed.pid)) {
                (Some(td), Some(&(prev_start, prev_ticks))) if prev_start == parsed.starttime => {
                    parsed
                        .ticks
                        .checked_sub(prev_ticks)
                        .map(|pd| ((pd as f64 / td as f64) * 100.0).clamp(0.0, 100.0) as f32)
                        .unwrap_or(0.0)
                }
                _ => 0.0,
            };
            samples.push(ProcessSample {
                pid: parsed.pid,
                name: parsed.name,
                cpu_pct,
                mem_bytes: parsed.rss_bytes,
            });
        }

        self.prev_total_ticks = total_ticks;
        self.prev_procs = next_procs;

        samples.sort_by_key(|a| std::cmp::Reverse(a.mem_bytes));
        if samples.len() > limit {
            samples.truncate(limit);
        }
        samples
    }
}

/// Enumerate processes in `proc_root` and return top processes sorted by memory usage.
pub fn top_processes(proc_root: &Path, limit: usize) -> Vec<ProcessSample> {
    ProcessMonitor::default().sample(proc_root, limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parse_stat_handles_spaces_and_parens_in_name() {
        let content = "1234 (Web Content (Worker)) S 1 1234 1234 0 -1 4194304 100 0 0 0 450 150 0 0 20 0 10 0 1000 500000 2500 18446744073709551615";
        let parsed = parse_proc_stat(content);
        assert!(parsed.is_some());
        let (pid, name, ticks, rss_bytes) = parsed.unwrap();
        assert_eq!(pid, 1234);
        assert_eq!(name, "Web Content (Worker)");
        assert_eq!(ticks, 600); // 450 + 150
        assert_eq!(rss_bytes, 2500 * 4096);
    }

    #[test]
    fn top_processes_scans_and_sorts_correctly() {
        let dir = tempdir().unwrap();
        let p1 = dir.path().join("101");
        let p2 = dir.path().join("102");
        let not_proc = dir.path().join("cpuinfo");
        fs::create_dir(&p1).unwrap();
        fs::create_dir(&p2).unwrap();
        fs::write(&not_proc, "dummy").unwrap();

        fs::write(
            p1.join("stat"),
            "101 (small_proc) S 1 1 1 0 -1 0 0 0 0 0 10 10 0 0 20 0 1 0 0 0 100 0",
        )
        .unwrap();
        fs::write(
            p2.join("stat"),
            "102 (big_proc) S 1 1 1 0 -1 0 0 0 0 0 20 20 0 0 20 0 1 0 0 0 500 0",
        )
        .unwrap();

        let top = top_processes(dir.path(), 5);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].pid, 102);
        assert_eq!(top[0].name, "big_proc");
        assert_eq!(top[0].mem_bytes, 500 * 4096);
        assert_eq!(top[1].pid, 101);
        assert_eq!(top[1].name, "small_proc");
        assert_eq!(top[1].mem_bytes, 100 * 4096);
    }

    #[test]
    fn kernel_threads_with_zero_rss_are_excluded() {
        let dir = tempdir().unwrap();
        let kthread = dir.path().join("2");
        let user = dir.path().join("200");
        fs::create_dir(&kthread).unwrap();
        fs::create_dir(&user).unwrap();
        fs::write(
            kthread.join("stat"),
            "2 (kthreadd) S 0 0 0 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 1 0 0 0",
        )
        .unwrap();
        fs::write(
            user.join("stat"),
            "200 (app) S 1 200 200 0 -1 0 0 0 0 0 10 10 0 0 20 0 1 0 50 0 300 0",
        )
        .unwrap();
        let top = top_processes(dir.path(), 5);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].pid, 200);
    }

    #[test]
    fn process_monitor_computes_cpu_percentage_and_handles_pid_reuse() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("500");
        fs::create_dir(&p).unwrap();
        fs::write(dir.path().join("stat"), "cpu  1000 0 0 1000 0 0 0 0 0 0\n").unwrap();
        fs::write(
            p.join("stat"),
            "500 (worker) S 1 500 500 0 -1 0 0 0 0 0 100 50 0 0 20 0 1 0 42 0 400 0",
        )
        .unwrap();

        let mut monitor = ProcessMonitor::default();
        let first = monitor.sample(dir.path(), 5);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].cpu_pct, 0.0);

        // Advance total CPU ticks by 1000, and worker utime+stime by 250 (25.0%).
        fs::write(dir.path().join("stat"), "cpu  2000 0 0 1000 0 0 0 0 0 0\n").unwrap();
        fs::write(
            p.join("stat"),
            "500 (worker) S 1 500 500 0 -1 0 0 0 0 0 300 100 0 0 20 0 1 0 42 0 400 0",
        )
        .unwrap();
        let second = monitor.sample(dir.path(), 5);
        assert!((second[0].cpu_pct - 25.0).abs() < 0.01);

        // Simulate PID reuse: same PID 500, different starttime (999 instead of 42).
        fs::write(dir.path().join("stat"), "cpu  3000 0 0 1000 0 0 0 0 0 0\n").unwrap();
        fs::write(
            p.join("stat"),
            "500 (new_proc) S 1 500 500 0 -1 0 0 0 0 0 10 10 0 0 20 0 1 0 999 0 400 0",
        )
        .unwrap();
        let reused = monitor.sample(dir.path(), 5);
        assert_eq!(reused[0].cpu_pct, 0.0);
    }
}
