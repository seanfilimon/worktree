//! Latency samples and their summary (count, errors, percentiles,
//! throughput).

use std::time::Duration;

use serde::Serialize;

/// Raw measurements of one operation kind.
#[derive(Debug, Clone, Default)]
pub struct Samples {
    /// Latency of every successful operation.
    pub latencies: Vec<Duration>,
    /// Failed operations (not included in the latencies).
    pub errors: u64,
    /// First few error messages, for the report.
    pub error_examples: Vec<String>,
    /// Payload bytes moved by the successful operations (0 = not
    /// meaningful for this operation).
    pub bytes: u64,
}

/// How many distinct error messages a report keeps.
const MAX_ERROR_EXAMPLES: usize = 3;

impl Samples {
    /// Record a success that took `d` and moved `bytes`.
    pub fn ok(&mut self, d: Duration, bytes: u64) {
        self.latencies.push(d);
        self.bytes += bytes;
    }

    /// Record a failure.
    pub fn err(&mut self, msg: impl Into<String>) {
        self.errors += 1;
        let msg = msg.into();
        if self.error_examples.len() < MAX_ERROR_EXAMPLES && !self.error_examples.contains(&msg) {
            self.error_examples.push(msg);
        }
    }

    /// Merge another worker's samples into these.
    pub fn merge(&mut self, other: Samples) {
        self.latencies.extend(other.latencies);
        self.bytes += other.bytes;
        self.errors += other.errors;
        for e in other.error_examples {
            if self.error_examples.len() < MAX_ERROR_EXAMPLES && !self.error_examples.contains(&e) {
                self.error_examples.push(e);
            }
        }
    }

    /// Summarize, with throughput relative to `wall` (the elapsed time of
    /// the phase that produced the samples).
    pub fn summarize(&self, op: &str, wall: Duration) -> Summary {
        let mut sorted = self.latencies.clone();
        sorted.sort_unstable();
        let ms = |d: Option<Duration>| d.map(|d| d.as_secs_f64() * 1e3).unwrap_or(0.0);
        let secs = wall.as_secs_f64();
        let count = sorted.len() as u64;
        let mean = if sorted.is_empty() {
            0.0
        } else {
            sorted.iter().map(|d| d.as_secs_f64()).sum::<f64>() * 1e3 / sorted.len() as f64
        };
        Summary {
            op: op.to_string(),
            count,
            errors: self.errors,
            mean_ms: mean,
            p50_ms: ms(percentile(&sorted, 50.0)),
            p95_ms: ms(percentile(&sorted, 95.0)),
            p99_ms: ms(percentile(&sorted, 99.0)),
            max_ms: ms(sorted.last().copied()),
            wall_s: secs,
            ops_per_s: if secs > 0.0 { count as f64 / secs } else { 0.0 },
            mb_per_s: if self.bytes > 0 && secs > 0.0 {
                Some(self.bytes as f64 / (1024.0 * 1024.0) / secs)
            } else {
                None
            },
            bytes: self.bytes,
            error_examples: self.error_examples.clone(),
        }
    }
}

/// The `p`-th percentile (0..=100) of ascending `sorted` samples by the
/// nearest-rank method: the smallest sample such that at least p % of the
/// samples are less than or equal to it. `None` for no samples.
pub fn percentile(sorted: &[Duration], p: f64) -> Option<Duration> {
    if sorted.is_empty() {
        return None;
    }
    let p = p.clamp(0.0, 100.0);
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

/// Report row of one operation kind.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub op: String,
    pub count: u64,
    pub errors: u64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    /// Elapsed time of the phase.
    pub wall_s: f64,
    /// Successful operations per second of the phase.
    pub ops_per_s: f64,
    /// Payload throughput, when the operation moves file content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mb_per_s: Option<f64>,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub error_examples: Vec<String>,
}

/// Render summaries as an aligned text table.
pub fn table(rows: &[Summary]) -> String {
    let header = [
        "op", "count", "errors", "p50 ms", "p95 ms", "p99 ms", "max ms", "mean ms", "ops/s", "MB/s",
    ];
    let mut cells: Vec<Vec<String>> = vec![header.iter().map(|s| s.to_string()).collect()];
    for r in rows {
        cells.push(vec![
            r.op.clone(),
            r.count.to_string(),
            r.errors.to_string(),
            format!("{:.1}", r.p50_ms),
            format!("{:.1}", r.p95_ms),
            format!("{:.1}", r.p99_ms),
            format!("{:.1}", r.max_ms),
            format!("{:.1}", r.mean_ms),
            format!("{:.2}", r.ops_per_s),
            r.mb_per_s
                .map(|m| format!("{m:.2}"))
                .unwrap_or_else(|| "-".into()),
        ]);
    }
    let widths: Vec<usize> = (0..header.len())
        .map(|c| cells.iter().map(|row| row[c].len()).max().unwrap_or(0))
        .collect();
    let mut out = String::new();
    for (i, row) in cells.iter().enumerate() {
        let line: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(c, v)| {
                if c == 0 {
                    format!("{v:<w$}", w = widths[c])
                } else {
                    format!("{v:>w$}", w = widths[c])
                }
            })
            .collect();
        out.push_str(line.join("  ").trim_end());
        out.push('\n');
        if i == 0 {
            let total = widths.iter().sum::<usize>() + 2 * (widths.len() - 1);
            out.push_str(&"-".repeat(total));
            out.push('\n');
        }
    }
    for r in rows {
        for e in &r.error_examples {
            out.push_str(&format!("{} error: {e}\n", r.op));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    fn sorted(vals: &[u64]) -> Vec<Duration> {
        let mut v: Vec<Duration> = vals.iter().map(|&x| ms(x)).collect();
        v.sort();
        v
    }

    #[test]
    fn percentile_of_nothing_is_none() {
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn percentile_single_sample() {
        let s = sorted(&[7]);
        for p in [0.0, 1.0, 50.0, 99.0, 100.0] {
            assert_eq!(percentile(&s, p), Some(ms(7)), "p{p}");
        }
    }

    #[test]
    fn percentile_nearest_rank() {
        // 1..=100 ms: the p-th percentile is p ms.
        let s = sorted(&(1..=100).collect::<Vec<_>>());
        assert_eq!(percentile(&s, 50.0), Some(ms(50)));
        assert_eq!(percentile(&s, 95.0), Some(ms(95)));
        assert_eq!(percentile(&s, 99.0), Some(ms(99)));
        assert_eq!(percentile(&s, 100.0), Some(ms(100)));
        assert_eq!(percentile(&s, 0.0), Some(ms(1)));
        // Small sets round the rank up.
        let s = sorted(&[10, 20, 30, 40]);
        assert_eq!(percentile(&s, 50.0), Some(ms(20)));
        assert_eq!(percentile(&s, 51.0), Some(ms(30)));
        assert_eq!(percentile(&s, 95.0), Some(ms(40)));
        // Out-of-range p is clamped.
        assert_eq!(percentile(&s, 250.0), Some(ms(40)));
        assert_eq!(percentile(&s, -3.0), Some(ms(10)));
    }

    #[test]
    fn summarize_computes_rates_and_percentiles() {
        let mut s = Samples::default();
        for v in [40, 10, 30, 20] {
            s.ok(ms(v), 1024 * 1024);
        }
        s.err("boom");
        s.err("boom");
        s.err("bang");
        let sum = s.summarize("push", Duration::from_secs(2));
        assert_eq!(sum.count, 4);
        assert_eq!(sum.errors, 3);
        assert_eq!(
            sum.error_examples,
            vec!["boom".to_string(), "bang".to_string()]
        );
        assert!((sum.p50_ms - 20.0).abs() < 1e-9);
        assert!((sum.p95_ms - 40.0).abs() < 1e-9);
        assert!((sum.max_ms - 40.0).abs() < 1e-9);
        assert!((sum.mean_ms - 25.0).abs() < 1e-9);
        assert!((sum.ops_per_s - 2.0).abs() < 1e-9);
        assert_eq!(sum.mb_per_s, Some(2.0));
    }

    #[test]
    fn summarize_empty_and_byteless() {
        let sum = Samples::default().summarize("search", Duration::ZERO);
        assert_eq!((sum.count, sum.errors), (0, 0));
        assert_eq!(sum.p99_ms, 0.0);
        assert_eq!(sum.ops_per_s, 0.0);
        assert_eq!(sum.mb_per_s, None);
        let mut s = Samples::default();
        s.ok(ms(5), 0);
        assert_eq!(s.summarize("search", Duration::from_secs(1)).mb_per_s, None);
    }

    #[test]
    fn merge_combines_workers() {
        let mut a = Samples::default();
        a.ok(ms(1), 10);
        a.err("x");
        let mut b = Samples::default();
        b.ok(ms(2), 5);
        b.ok(ms(3), 5);
        b.err("x");
        b.err("y");
        a.merge(b);
        assert_eq!(a.latencies.len(), 3);
        assert_eq!(a.bytes, 20);
        assert_eq!(a.errors, 3);
        assert_eq!(a.error_examples, vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn table_renders_rows() {
        let mut s = Samples::default();
        s.ok(ms(12), 0);
        let t = table(&[s.summarize("search", Duration::from_secs(1))]);
        let lines: Vec<&str> = t.lines().collect();
        assert!(lines[0].starts_with("op"), "{t}");
        assert!(lines[1].starts_with("---"), "{t}");
        assert!(lines[2].starts_with("search"), "{t}");
        assert!(lines[2].contains("12.0"), "{t}");
        assert!(
            lines[2].trim_end().ends_with('-'),
            "no MB/s for searches: {t}"
        );
    }
}
