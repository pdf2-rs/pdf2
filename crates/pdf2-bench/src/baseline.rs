//! The committed performance baseline and the regression check.
//!
//! Timings are only comparable on the machine that produced them, so the file
//! stores one entry per `os-arch` and `--check` compares against the entry for
//! the current platform. Allocation counts are deterministic for the same input
//! and are compared against any entry, which is what makes the gate meaningful
//! on a CI runner that has no timing baseline yet.

use std::collections::BTreeMap;
use std::path::Path;

use crate::json::{self, Json};
use crate::metrics::{Metrics, lower_is_better};

/// How much a metric may change before it counts as a regression.
pub const DEFAULT_TOLERANCE: f64 = 0.10;

/// A loaded baseline file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Baseline {
    /// When the file was last written.
    pub generated: String,
    /// One entry per platform.
    pub entries: Vec<Entry>,
}

/// The measurements recorded for one platform.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// `os-arch`, matching [`crate::metrics::platform`].
    pub platform: String,
    /// Metric key to value.
    pub values: BTreeMap<String, f64>,
}

/// The outcome of comparing a run against the baseline.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Metrics that were compared, with both numbers.
    pub compared: Vec<(String, f64, f64)>,
    /// Metrics with no baseline value to compare against.
    pub skipped: Vec<String>,
    /// Metrics that moved past the tolerance.
    pub regressions: Vec<(String, f64, f64)>,
}

impl Report {
    /// Whether the run is within tolerance.
    pub fn is_ok(&self) -> bool {
        self.regressions.is_empty()
    }
}

impl Baseline {
    /// An empty baseline.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a baseline file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let document =
            json::parse(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        let generated = document
            .get("generated")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned();
        let entries = document
            .get("entries")
            .and_then(Json::as_array)
            .ok_or_else(|| format!("{}: no `entries` array", path.display()))?
            .iter()
            .map(entry_from_json)
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self { generated, entries })
    }

    /// Write the baseline, one metric per line so diffs stay readable.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut out = String::from("{\n");
        out.push_str(&format!(
            "  \"generated\": {},\n",
            json::string(&self.generated)
        ));
        out.push_str("  \"entries\": [\n");
        let entries: Vec<String> = self.entries.iter().map(entry_text).collect();
        out.push_str(&entries.join(",\n"));
        out.push_str("\n  ]\n}\n");
        std::fs::write(path, out).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Insert or replace the entry for `platform`.
    pub fn upsert(&mut self, platform: &str, values: BTreeMap<String, f64>) {
        match self
            .entries
            .iter_mut()
            .find(|entry| entry.platform == platform)
        {
            Some(entry) => entry.values = values,
            None => self.entries.push(Entry {
                platform: platform.to_owned(),
                values,
            }),
        }
    }

    /// Compare a run against the baseline.
    pub fn check(&self, metrics: &Metrics, tolerance: f64) -> Report {
        let same_platform = self
            .entries
            .iter()
            .find(|entry| entry.platform == metrics.platform);
        let mut report = Report::default();
        for (name, current) in &metrics.values {
            if is_informational(name) {
                report.skipped.push(name.clone());
                continue;
            }
            let reference = same_platform
                .and_then(|entry| entry.values.get(name).copied())
                .or_else(|| {
                    if is_deterministic(name) {
                        self.entries
                            .iter()
                            .find_map(|entry| entry.values.get(name).copied())
                    } else {
                        None
                    }
                });
            let Some(reference) = reference else {
                report.skipped.push(name.clone());
                continue;
            };
            report.compared.push((name.clone(), reference, *current));
            let regressed = if lower_is_better(name) {
                *current > reference * (1.0 + tolerance)
            } else {
                *current < reference * (1.0 - tolerance)
            };
            if regressed {
                report.regressions.push((name.clone(), reference, *current));
            }
        }
        report
    }
}

/// Allocation counts do not depend on the platform, so they are checked even
/// when the current platform has no entry of its own.
fn is_deterministic(name: &str) -> bool {
    name.ends_with(".allocations") || name.ends_with(".alloc_bytes")
}

/// Metrics that are recorded and printed but never gated: peak RSS depends on
/// the allocator's arena state as much as on this crate, while the allocation
/// counts are the deterministic memory guard.
fn is_informational(name: &str) -> bool {
    name.ends_with(".peak_rss_kib")
}

fn entry_text(entry: &Entry) -> String {
    let mut lines = vec![format!(
        "      \"platform\": {}",
        json::string(&entry.platform)
    )];
    for (name, value) in &entry.values {
        lines.push(format!(
            "      {}: {}",
            json::string(name),
            json::number(*value)
        ));
    }
    format!("    {{\n{}\n    }}", lines.join(",\n"))
}

fn entry_from_json(value: &Json) -> Result<Entry, String> {
    let Json::Object(entries) = value else {
        return Err("baseline entry is not an object".to_owned());
    };
    let mut platform = None;
    let mut values = BTreeMap::new();
    for (key, value) in entries {
        if key == "platform" {
            platform = value.as_str().map(str::to_owned);
        } else if let Some(number) = value.as_f64() {
            values.insert(key.clone(), number);
        }
    }
    Ok(Entry {
        platform: platform.ok_or("baseline entry without a platform")?,
        values,
    })
}

#[cfg(test)]
mod tests {
    use super::{Baseline, DEFAULT_TOLERANCE, Entry, Report};
    use crate::metrics::Metrics;
    use std::collections::BTreeMap;

    fn metrics(values: &[(&str, f64)]) -> Metrics {
        Metrics {
            platform: "test-platform".to_owned(),
            values: values
                .iter()
                .map(|(name, value)| ((*name).to_owned(), *value))
                .collect(),
        }
    }

    fn baseline(values: &[(&str, f64)]) -> Baseline {
        let mut entry_values = BTreeMap::new();
        for (name, value) in values {
            entry_values.insert((*name).to_owned(), *value);
        }
        Baseline {
            generated: "2026-11-14".to_owned(),
            entries: vec![Entry {
                platform: "test-platform".to_owned(),
                values: entry_values,
            }],
        }
    }

    #[test]
    fn throughput_regressions_are_flagged() {
        let baseline = baseline(&[("g.read_mb_s", 100.0)]);
        let report = baseline.check(&metrics(&[("g.read_mb_s", 89.0)]), DEFAULT_TOLERANCE);
        assert_eq!(report.regressions.len(), 1);
        assert!(!report.is_ok());

        let report = baseline.check(&metrics(&[("g.read_mb_s", 95.0)]), DEFAULT_TOLERANCE);
        assert!(report.is_ok(), "{report:?}");
    }

    #[test]
    fn allocation_growth_is_a_regression() {
        let baseline = baseline(&[("g.allocations", 1000.0)]);
        assert!(
            !baseline
                .check(&metrics(&[("g.allocations", 1200.0)]), DEFAULT_TOLERANCE)
                .is_ok()
        );
        assert!(
            baseline
                .check(&metrics(&[("g.allocations", 1050.0)]), DEFAULT_TOLERANCE)
                .is_ok()
        );
    }

    #[test]
    fn metrics_without_a_reference_are_skipped_and_deterministic_ones_cross_check() {
        let mut baseline = baseline(&[("g.read_mb_s", 100.0), ("g.allocations", 1000.0)]);
        baseline.entries[0].platform = "other-platform".to_owned();
        let report = baseline.check(
            &metrics(&[("g.read_mb_s", 10.0), ("g.allocations", 2000.0)]),
            DEFAULT_TOLERANCE,
        );
        assert_eq!(report.skipped, vec!["g.read_mb_s".to_owned()]);
        assert_eq!(report.regressions.len(), 1, "{report:?}");
        assert_eq!(report.regressions[0].0, "g.allocations");
    }

    #[test]
    fn upsert_replaces_the_entry_for_a_platform() {
        let mut baseline = baseline(&[("g.read_mb_s", 100.0)]);
        let mut values = BTreeMap::new();
        values.insert("g.read_mb_s".to_owned(), 200.0);
        values.insert("g.allocations".to_owned(), 7.0);
        baseline.upsert("test-platform", values);
        assert_eq!(baseline.entries.len(), 1);
        assert_eq!(
            baseline.entries[0].values.get("g.read_mb_s").copied(),
            Some(200.0)
        );

        let mut other = BTreeMap::new();
        other.insert("g.read_mb_s".to_owned(), 1.0);
        baseline.upsert("another-platform", other);
        assert_eq!(baseline.entries.len(), 2);
    }

    #[test]
    fn saving_and_loading_round_trips() {
        let mut path = std::env::temp_dir();
        path.push(format!("pdf2-bench-baseline-{}.json", std::process::id()));
        let mut baseline = baseline(&[("g.read_mb_s", 100.5), ("g.allocations", 1000.0)]);
        baseline.upsert(
            "second-platform",
            [("g.open_mb_s".to_owned(), 42.0)].into_iter().collect(),
        );
        baseline.generated = "2026-11-14".to_owned();
        baseline.save(&path).expect("save");
        let loaded = Baseline::load(&path).expect("load");
        assert_eq!(loaded, baseline);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn peak_memory_is_informational() {
        let baseline = baseline(&[("process.peak_rss_kib", 100.0)]);
        let report = baseline.check(
            &metrics(&[("process.peak_rss_kib", 900.0)]),
            DEFAULT_TOLERANCE,
        );
        assert!(report.is_ok(), "{report:?}");
        assert_eq!(report.skipped, vec!["process.peak_rss_kib".to_owned()]);
    }

    #[test]
    fn an_empty_report_is_ok() {
        assert!(Report::default().is_ok());
    }
}
