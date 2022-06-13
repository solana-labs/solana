// SPDX-FileCopyrightText: 2022 Chorus One AG
// SPDX-License-Identifier: Apache-2.0

//! Utilities for formatting Prometheus metrics.
//!
//! See also <https://prometheus.io/docs/instrumenting/exposition_formats/#text-based-format>.

use std::io;
use std::io::Write;
use std::time::SystemTime;

use crate::Lamports;

pub struct MetricFamily<'a> {
    /// Name of the metric, e.g. [`goats_teleported_total`](https://crbug.com/31482).
    pub name: &'a str,
    /// HELP line content.
    pub help: &'a str,
    /// TYPE line content. Most common are `counter`, `gauge`, and `histogram`.
    pub type_: &'a str,
    /// Values for this metric, possibly with labels or a suffix.
    pub metrics: Vec<Metric<'a>>,
}

pub enum MetricValue {
    /// Render the inner value as-is, as an integer.
    Int(u64),

    /// Divide the inner value by 10<sup>9</sup> and render as fixed-point number.
    ///
    /// E.g. `Nano(12)` renders as `0.000000012`.
    Nano(u64),

    Float(f64),
}

impl From<u64> for MetricValue {
    fn from(v: u64) -> MetricValue {
        MetricValue::Int(v)
    }
}

impl From<f64> for MetricValue {
    fn from(v: f64) -> MetricValue {
        MetricValue::Float(v)
    }
}

pub struct Metric<'a> {
    /// Suffix to append to the metric name, useful for e.g. the `_bucket` suffix on histograms.
    pub suffix: &'a str,

    /// Name-value label pairs.
    pub labels: Vec<(&'a str, String)>,

    /// Metric value, either an integer, or a fixed-point number.
    pub value: MetricValue,

    /// Time at which this metric was observed, when proxying metrics.
    pub timestamp: Option<SystemTime>,
}

impl<'a> Metric<'a> {
    /// Construct a basic metric with just a value.
    ///
    /// Can be extended with the builder-style methods below.
    pub fn new<T: Into<MetricValue>>(value: T) -> Metric<'a> {
        Metric {
            labels: Vec::new(),
            suffix: "",
            value: value.into(),
            timestamp: None,
        }
    }

    /// Construct a metric that measures an amount of SOL.
    pub fn new_sol(amount: Lamports) -> Metric<'a> {
        // One Lamport is 1e-9 SOL, so we use nano here.
        Metric::new(MetricValue::Nano(amount.0))
    }

    pub fn with_label(mut self, label_key: &'a str, label_value: String) -> Metric<'a> {
        self.labels.push((label_key, label_value));
        self
    }
}

pub fn write_metric<W: Write>(out: &mut W, family: &MetricFamily) -> io::Result<()> {
    writeln!(out, "# HELP {} {}", family.name, family.help)?;
    writeln!(out, "# TYPE {} {}", family.name, family.type_)?;
    for metric in &family.metrics {
        write!(out, "{}{}", family.name, metric.suffix)?;

        // If there are labels, write the key-value pairs between {}.
        // Escaping of the value uses Rust's string syntax, which is
        // not exactly what Prometheus wants, but it is identical for
        // all of the values that we use it with; this is not a general
        // Prometheus formatter, just a quick one for our use.
        if !metric.labels.is_empty() {
            write!(out, "{{")?;
            let mut separator = "";
            for (key, value) in &metric.labels {
                write!(out, "{}{}={:?}", separator, key, value)?;
                separator = ",";
            }
            write!(out, "}}")?;
        }

        match metric.value {
            MetricValue::Int(v) => write!(out, " {}", v)?,
            MetricValue::Nano(v) => {
                write!(out, " {}.{:0>9}", v / 1_000_000_000, v % 1_000_000_000)?
            }
            MetricValue::Float(v) => write!(out, " {}", v)?,
        }

        if let Some(timestamp) = metric.timestamp {
            let unix_time_ms = match timestamp.duration_since(SystemTime::UNIX_EPOCH) {
                Ok(duration) => duration.as_millis(),
                Err(..) => panic!("Found a metric dated before UNIX_EPOCH."),
            };
            // Timestamps in Prometheus are milliseconds since epoch,
            // excluding leap seconds. (Which is what you get if your system
            // clock tracks UTC.)
            write!(out, " {}", unix_time_ms)?;
        }

        writeln!(out)?;
    }

    // Add a blank line for readability by humans.
    writeln!(out)
}

#[cfg(test)]
mod test {
    use std::str;

    use super::{write_metric, Metric, MetricFamily, MetricValue};

    #[test]
    fn write_metric_without_labels() {
        let mut out: Vec<u8> = Vec::new();
        write_metric(
            &mut out,
            &MetricFamily {
                // The metric names are just for testing purposes.
                // See also https://crbug.com/31482.
                name: "goats_teleported_total",
                help: "Number of goats teleported since launch.",
                type_: "counter",
                metrics: vec![Metric::new(144)],
            },
        )
        .unwrap();

        assert_eq!(
            str::from_utf8(&out[..]),
            Ok(
                "# HELP goats_teleported_total Number of goats teleported since launch.\n\
                 # TYPE goats_teleported_total counter\n\
                 goats_teleported_total 144\n\n\
                "
            )
        )
    }

    #[test]
    fn write_metric_histogram() {
        let mut out: Vec<u8> = Vec::new();
        write_metric(
            &mut out,
            &MetricFamily {
                name: "teleported_goat_weight_kg",
                help: "Histogram of the weight of teleported goats.",
                type_: "histogram",
                metrics: vec![
                    Metric::new(44)
                        .with_suffix("_bucket")
                        .with_label("le", "50.0".to_string()),
                    Metric::new(67)
                        .with_suffix("_bucket")
                        .with_label("le", "75.0".to_string()),
                    Metric::new(144)
                        .with_suffix("_bucket")
                        .with_label("le", "+Inf".to_string()),
                    Metric::new(11520).with_suffix("_sum"),
                    Metric::new(144).with_suffix("_count"),
                ],
            },
        )
        .unwrap();

        assert_eq!(
            str::from_utf8(&out[..]),
            Ok(
                "# HELP teleported_goat_weight_kg Histogram of the weight of teleported goats.\n\
                 # TYPE teleported_goat_weight_kg histogram\n\
                 teleported_goat_weight_kg_bucket{le=\"50.0\"} 44\n\
                 teleported_goat_weight_kg_bucket{le=\"75.0\"} 67\n\
                 teleported_goat_weight_kg_bucket{le=\"+Inf\"} 144\n\
                 teleported_goat_weight_kg_sum 11520\n\
                 teleported_goat_weight_kg_count 144\n\n\
                "
            )
        )
    }

    #[test]
    fn write_metric_multiple_labels() {
        let mut out: Vec<u8> = Vec::new();
        write_metric(
            &mut out,
            &MetricFamily {
                name: "goats_teleported_total",
                help: "Number of goats teleported since launch by departure and arrival.",
                type_: "counter",
                metrics: vec![
                    Metric::new(10)
                        .with_label("src", "AMS".to_string())
                        .with_label("dst", "ZRH".to_string()),
                    Metric::new(53)
                        .with_label("src", "ZRH".to_string())
                        .with_label("dst", "DXB".to_string()),
                ],
            },
        )
        .unwrap();

        assert_eq!(
            str::from_utf8(&out[..]),
            Ok(
                "# HELP goats_teleported_total Number of goats teleported since launch by departure and arrival.\n\
                 # TYPE goats_teleported_total counter\n\
                 goats_teleported_total{src=\"AMS\",dst=\"ZRH\"} 10\n\
                 goats_teleported_total{src=\"ZRH\",dst=\"DXB\"} 53\n\n\
                "
            )
        )
    }

    #[test]
    fn write_metric_with_timestamp() {
        use std::time::{Duration, SystemTime};

        let mut out: Vec<u8> = Vec::new();
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(77);
        write_metric(
            &mut out,
            &MetricFamily {
                name: "goats_teleported_total",
                help: "Number of goats teleported since launch.",
                type_: "counter",
                metrics: vec![Metric::new(10).at(t)],
            },
        )
        .unwrap();

        assert_eq!(
            str::from_utf8(&out[..]),
            Ok(
                "# HELP goats_teleported_total Number of goats teleported since launch.\n\
                 # TYPE goats_teleported_total counter\n\
                 goats_teleported_total 10 77000\n\n\
                "
            )
        )
    }

    #[test]
    fn write_metric_nano_micro() {
        let mut out: Vec<u8> = Vec::new();
        write_metric(
            &mut out,
            &MetricFamily {
                name: "goat_weight_kg",
                help: "Weight of the goat in kilograms.",
                type_: "gauge",
                metrics: vec![
                    // One greater than 1, with no need for zero padding.
                    Metric::new(MetricValue::Nano(67_533_128_017)),
                    // One smaller than 1, with the need for zero padding.
                    Metric::new(MetricValue::Nano(128_017)),
                ],
            },
        )
        .unwrap();

        assert_eq!(
            str::from_utf8(&out[..]),
            Ok("# HELP goat_weight_kg Weight of the goat in kilograms.\n\
                 # TYPE goat_weight_kg gauge\n\
                 goat_weight_kg 67.533128017\n\
                 goat_weight_kg 67.533128\n\
                 goat_weight_kg 0.000128017\n\
                 goat_weight_kg 0.000128\n\n\
                ")
        )
    }
}
