//! Process-level resident-memory ground truth for the memory benchmark.
//!
//! The kernel's counters are authoritative — not allocator or JS-heap numbers
//! (memory-benchmark ticket 01). Method matches `docs/benchmarks.md` §Memory:
//!
//! - Linux: `VmRSS` / `VmHWM` from `/proc/self/status`. `VmHWM` is the
//!   kernel-recorded all-time peak, capturing mid-call highs that sampling
//!   can't observe (the method the container baseline uses).
//! - Windows: working-set counters from `GetProcessMemoryInfo`
//!   (`K32GetProcessMemoryInfo`, kernel32) — `WorkingSetSize` ≈ VmRSS,
//!   `PeakWorkingSetSize` ≈ VmHWM.
//!
//! Peak counters are monotonic per process; each scale cell therefore runs in
//! its own child process so peaks measure that cell alone.

/// One observation of the process's resident footprint in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    /// Current resident set (VmRSS / Windows working set).
    pub rss: u64,
    /// All-time peak resident set (VmHWM / peak working set).
    pub peak: u64,
    /// Committed (private) bytes — Windows commit charge (pagefile usage);
    /// Linux `VmData`. Unlike the resident set, commit drops when memory is
    /// *actually* freed, so flat commit under a climbing RSS means the
    /// allocator is holding freed pages resident, not leaking.
    pub commit: u64,
}

#[cfg(target_os = "linux")]
mod imp {
    use std::fs;

    pub(crate) fn status_field_kib(status: &str, name: &str) -> Option<u64> {
        // No regex dependency: match the line prefix directly. `/proc/self/status`
        // lines are `FieldName:<tab><value> kB`, so the colon after the name is
        // stripped too (memory-benchmark ticket 03) — otherwise `split_whitespace`
        // yields `:` and the parse fails.
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix(name) {
                if let Some(rest) = rest.strip_prefix(':') {
                    let rest = rest.trim_start();
                    if let Some(number) = rest.split_whitespace().next() {
                        return number.parse::<u64>().ok().map(|kib| kib * 1024);
                    }
                }
            }
        }
        None
    }

    pub fn snapshot() -> Option<super::Snapshot> {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        Some(super::Snapshot {
            rss: status_field_kib(&status, "VmRSS")?,
            peak: status_field_kib(&status, "VmHWM")?,
            commit: status_field_kib(&status, "VmData").unwrap_or(0),
        })
    }
}

#[cfg(windows)]
mod imp {
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    pub fn snapshot() -> Option<super::Snapshot> {
        let mut counters = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            page_fault_count: 0,
            peak_working_set_size: 0,
            working_set_size: 0,
            quota_peak_paged_pool_usage: 0,
            quota_paged_pool_usage: 0,
            quota_peak_non_paged_pool_usage: 0,
            quota_non_paged_pool_usage: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
        };
        let ok = unsafe {
            K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb)
        };
        if ok == 0 {
            return None;
        }
        Some(super::Snapshot {
            rss: counters.working_set_size as u64,
            peak: counters.peak_working_set_size as u64,
            commit: counters.pagefile_usage as u64,
        })
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    pub fn snapshot() -> Option<super::Snapshot> {
        None
    }
}

impl Snapshot {
    /// A zero snapshot for platforms without counter support.
    pub fn zero() -> Snapshot {
        Snapshot {
            rss: 0,
            peak: 0,
            commit: 0,
        }
    }
}

/// Observe the current process's resident footprint; `None` when the
/// platform has no supported counter source.
pub fn snapshot() -> Option<Snapshot> {
    imp::snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "linux", windows))]
    fn returns_positive_counters_on_supported_platforms() {
        let snap = snapshot().expect("supported platform must produce a snapshot");
        assert!(snap.rss > 0);
        assert!(snap.peak >= snap.rss);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn parses_proc_status_fields_with_unit_suffix() {
        // `/proc/self/status` lines are `FieldName:<tab><value> kB` — the
        // colon after the name must be skipped or `split_whitespace` yields
        // `:` and the parse fails (memory-benchmark ticket 03: the Linux
        // snapshot silently returned `None` -> zeros, invalidating every
        // in-container RSS reading until a Linux run caught it).
        let sample =
            "Name:\ttest\nVmRSS:\t    1956 kB\nVmHWM:\t    3900 kB\nVmData:\t  123 kB\n";
        assert_eq!(imp::status_field_kib(sample, "VmRSS"), Some(1956 * 1024));
        assert_eq!(imp::status_field_kib(sample, "VmHWM"), Some(3900 * 1024));
        assert_eq!(imp::status_field_kib(sample, "VmData"), Some(123 * 1024));
        assert_eq!(imp::status_field_kib(sample, "Missing"), None);
        // The name prefix must not match a longer field name.
        assert_eq!(imp::status_field_kib("VmRSSFoo:\t1 kB\n", "VmRSS"), None);
    }
}
