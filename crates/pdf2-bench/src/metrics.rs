//! Measurement primitives: medians of repeated runs, allocation snapshots and
//! peak resident memory.

use std::collections::BTreeMap;
use std::process::Command;
use std::time::Instant;

use pdf2::Document;

use crate::alloc;
use crate::corpus::Group;

/// Everything one benchmark run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    /// `os-arch`, e.g. `macos-aarch64` or `linux-x86_64`.
    pub platform: String,
    /// Metric key to value, ordered for stable output.
    pub values: BTreeMap<String, f64>,
}

/// How often each timed operation is repeated before taking the median.
pub const DEFAULT_RUNS: usize = 7;

/// Measure every group and the process-level numbers.
pub fn collect(groups: &[Group], runs: usize) -> Result<Metrics, String> {
    let mut values = BTreeMap::new();
    for group in groups {
        let measured = measure_group(group, runs.max(1))?;
        for (suffix, value) in [
            ("read_mb_s", measured.read_mb_s),
            ("pages_s", measured.pages_s),
            ("text_pages_s", measured.text_pages_s),
            ("allocations", measured.allocations as f64),
            ("alloc_bytes", measured.alloc_bytes as f64),
        ] {
            values.insert(format!("{}.{suffix}", group.name), value);
        }
    }
    if let Some(peak) = peak_rss_kib() {
        values.insert("process.peak_rss_kib".to_owned(), peak as f64);
    }
    Ok(Metrics {
        platform: platform(),
        values,
    })
}

/// The current `os-arch`, matching the platform key in the baseline file.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

struct GroupMeasurement {
    /// Throughput of the whole read pipeline: open, page walk with content
    /// decoding, and text extraction.
    read_mb_s: f64,
    pages_s: f64,
    text_pages_s: f64,
    allocations: u64,
    alloc_bytes: u64,
}

fn measure_group(group: &Group, runs: usize) -> Result<GroupMeasurement, String> {
    let bytes = group.bytes();
    let (pages, allocations, alloc_bytes) = allocation_pass(group)?;
    let read = median_timing(runs, || {
        for file in &group.files {
            let document = Document::open(file.clone()).map_err(|error| error.to_string())?;
            let pages = document.pages().map_err(|error| error.to_string())?;
            for page in &pages {
                document
                    .page_content(page)
                    .map_err(|error| error.to_string())?;
                document
                    .extract_text(page)
                    .map_err(|error| error.to_string())?;
            }
            std::hint::black_box(&document);
        }
        Ok(())
    })?;
    let walk = median_timing(runs, || {
        for file in &group.files {
            let document = Document::open(file.clone()).map_err(|error| error.to_string())?;
            let pages = document.pages().map_err(|error| error.to_string())?;
            for page in &pages {
                document
                    .page_content(page)
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    })?;
    let extraction = median_timing(runs, || {
        for file in &group.files {
            let document = Document::open(file.clone()).map_err(|error| error.to_string())?;
            let pages = document.pages().map_err(|error| error.to_string())?;
            for page in &pages {
                document
                    .extract_text(page)
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    })?;
    let pages = pages.max(1) as f64;
    let megabytes = bytes as f64 / 1_000_000.0;
    Ok(GroupMeasurement {
        read_mb_s: rate(megabytes, read),
        pages_s: rate(pages, walk),
        text_pages_s: rate(pages, extraction),
        allocations,
        alloc_bytes,
    })
}

/// One pass that only counts allocations, after a counter reset.
fn allocation_pass(group: &Group) -> Result<(u64, u64, u64), String> {
    alloc::reset();
    let mut pages = 0_u64;
    for file in &group.files {
        let document = Document::open(file.clone()).map_err(|error| error.to_string())?;
        let page_list = document.pages().map_err(|error| error.to_string())?;
        pages += page_list.len() as u64;
        for page in &page_list {
            document
                .page_content(page)
                .map_err(|error| error.to_string())?;
            document
                .extract_text(page)
                .map_err(|error| error.to_string())?;
        }
    }
    let (allocations, alloc_bytes) = alloc::snapshot();
    Ok((pages, allocations, alloc_bytes))
}

/// Run `operation` `runs` times and return the median duration per operation.
///
/// Fast operations are batched: after one calibration run the operation is
/// repeated enough times to make each sample last at least [`MIN_SAMPLE`]
/// seconds, and the elapsed time is divided back down. Without that, opening a
/// 500-byte fixture would be a few microseconds and the median would swing
/// with scheduler noise.
fn median_timing(
    runs: usize,
    mut operation: impl FnMut() -> Result<(), String>,
) -> Result<f64, String> {
    const MIN_SAMPLE: f64 = 0.02;
    const MAX_REPEATS: usize = 10_000;
    let start = Instant::now();
    operation()?;
    let first = start.elapsed().as_secs_f64();
    let repeats = if first <= 0.0 {
        1
    } else {
        ((MIN_SAMPLE / first).ceil() as usize).clamp(1, MAX_REPEATS)
    };
    // One warm-up sample is thrown away: it carries the first-touch page faults
    // and cache misses that have nothing to do with the code under test.
    let start = Instant::now();
    for _ in 0..repeats {
        operation()?;
    }
    std::hint::black_box(start.elapsed());
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        for _ in 0..repeats {
            operation()?;
        }
        samples.push(start.elapsed().as_secs_f64() / repeats as f64);
    }
    Ok(median(&mut samples))
}

fn median(samples: &mut [f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.sort_by(|left, right| left.total_cmp(right));
    let middle = samples.len() / 2;
    if samples.len().is_multiple_of(2) {
        (samples[middle - 1] + samples[middle]) / 2.0
    } else {
        samples[middle]
    }
}

fn rate(units: f64, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        return 0.0;
    }
    units / seconds
}

/// Peak resident memory in KiB.
///
/// - Linux: `VmHWM` from `/proc/self/status` is the real high-water mark; the
///   current RSS from `/proc/self/statm` is the fallback.
/// - macOS: `ps -o rss=` reports the *current* resident size, not the peak,
///   because the high-water mark is not exposed without task APIs.
/// - Anywhere else: `None`, and the metric is simply absent.
pub fn peak_rss_kib() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        if let Some(value) = read_status_kib("/proc/self/status", "VmHWM:") {
            return Some(value);
        }
        return read_statm_kib("/proc/self/statm");
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn read_status_kib(path: &str, key: &str) -> Option<u64> {
    let status = std::fs::read_to_string(path).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn read_statm_kib(path: &str) -> Option<u64> {
    let statm = std::fs::read_to_string(path).ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // Linux reports pages here; 4096 is the page size of the platforms this
    // fallback is expected on, and it is only used when VmHWM is missing.
    Some(pages * 4096 / 1024)
}

/// Render a metrics map as a table, one line per metric.
pub fn table(metrics: &Metrics) -> String {
    let mut out = format!("platform: {}\n", metrics.platform);
    for (name, value) in &metrics.values {
        let rendered = if name.ends_with(".allocations") || name.ends_with(".alloc_bytes") {
            format!("{value:.0}")
        } else {
            format!("{value:.1}")
        };
        out.push_str(&format!("{name:<32} {rendered}\n"));
    }
    out
}

/// Whether a metric should be small (true) or large (false).
pub fn lower_is_better(name: &str) -> bool {
    name.ends_with(".allocations")
        || name.ends_with(".alloc_bytes")
        || name.ends_with(".peak_rss_kib")
}

#[cfg(test)]
mod tests {
    use super::{median, platform, rate};

    #[test]
    fn medians_are_taken_over_the_samples() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&mut [4.0, 1.0, 3.0, 2.0]), 2.5);
        assert_eq!(median(&mut [7.0]), 7.0);
        assert_eq!(median(&mut []), 0.0);
    }

    #[test]
    fn rates_are_units_per_second_and_survive_a_zero_duration() {
        assert_eq!(rate(10.0, 2.0), 5.0);
        assert_eq!(rate(10.0, 0.0), 0.0);
    }

    #[test]
    fn the_platform_key_is_os_arch() {
        let key = platform();
        assert!(key.contains('-'), "{key}");
        assert!(key.starts_with(std::env::consts::OS), "{key}");
    }

    #[test]
    fn peak_memory_is_read_on_this_platform() {
        let peak = super::peak_rss_kib();
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            assert!(peak.unwrap_or(0) > 0, "{peak:?}");
        }
    }
}
