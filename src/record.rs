//! bensho's own columns, the calibration rule and the report.

use std::fmt;

use crate::Options;

/// bensho's own columns, in CSV order. `Record`'s field names verbatim; the
/// row's columns follow as `data.<field>`.
pub const COLUMNS: &[&str] = &[
    "suite",
    "group",
    "name",
    "round",
    "position",
    "group_position",
    "cells",
    "seed",
    "batch",
    "ops",
    "elapsed_ns",
    "ns_per_op",
    "calibration",
    "pilot_ns_per_op",
    "start_ms",
];

/// A cell's path within its suite: `<group>/<name>`, or `<name>` for a
/// singleton (empty group).
pub fn cell_path(group: &str, name: &str) -> String {
    if group.is_empty() {
        name.to_string()
    } else {
        format!("{group}/{name}")
    }
}

/// How a cell's batch size was fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calibration {
    /// The ceiling fit the budget; batch is `ops`.
    Full,
    /// Scaled down to the budget.
    Budget,
    /// Even the floor exceeds the budget; batch is `min_ops` and rounds run long.
    Floor,
}

impl fmt::Display for Calibration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Calibration::Full => "Full",
            Calibration::Budget => "Budget",
            Calibration::Floor => "Floor",
        })
    }
}

/// The batch size for a cell whose pilot cost `pilot_ns_per_request` per
/// requested op. The rule the spec states, as a pure function.
pub fn calibrate(pilot_ns_per_request: f64, o: &Options) -> (usize, Calibration) {
    let budget_ns = o.budget.as_nanos() as f64;
    let fit = if pilot_ns_per_request > 0.0 {
        (budget_ns / pilot_ns_per_request).floor()
    } else {
        f64::INFINITY
    };
    if fit >= o.ops as f64 {
        (o.ops, Calibration::Full)
    } else if fit >= o.min_ops as f64 {
        (fit as usize, Calibration::Budget)
    } else {
        (o.min_ops.min(o.ops), Calibration::Floor)
    }
}

/// bensho's columns for one cell in one round.
#[derive(Clone, Debug)]
pub struct Record {
    pub suite: String,
    /// The group's name; empty for a singleton cell.
    pub group: String,
    pub name: String,
    pub round: u32,
    /// Slot within the round's flattened order.
    pub position: usize,
    /// Slot of the cell's group within the round's group order.
    pub group_position: usize,
    pub cells: usize,
    pub seed: u64,
    pub batch: usize,
    pub ops: u64,
    pub elapsed_ns: u128,
    pub ns_per_op: f64,
    pub calibration: Calibration,
    pub pilot_ns_per_op: f64,
    pub start_ms: u128,
}

impl Record {
    /// `<group>/<name>`, or `<name>` for a singleton.
    pub fn path(&self) -> String {
        cell_path(&self.group, &self.name)
    }

    /// The values in `COLUMNS` order.
    pub fn values(&self) -> Vec<String> {
        vec![
            self.suite.clone(),
            self.group.clone(),
            self.name.clone(),
            self.round.to_string(),
            self.position.to_string(),
            self.group_position.to_string(),
            self.cells.to_string(),
            self.seed.to_string(),
            self.batch.to_string(),
            self.ops.to_string(),
            self.elapsed_ns.to_string(),
            format!("{:.3}", self.ns_per_op),
            self.calibration.to_string(),
            format!("{:.3}", self.pilot_ns_per_op),
            self.start_ms.to_string(),
        ]
    }
}

/// One cell's calibration verdict.
#[derive(Clone, Debug)]
pub struct Calibrated {
    pub group: String,
    pub name: String,
    pub batch: usize,
    pub calibration: Calibration,
    pub pilot_ns_per_op: f64,
}

impl Calibrated {
    pub fn path(&self) -> String {
        cell_path(&self.group, &self.name)
    }
}

/// Everything a suite produced, for tests and in-process consumers.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub suite: String,
    pub records: Vec<Record>,
    pub calibrations: Vec<Calibrated>,
}

impl Report {
    /// The `== <suite>` heading, then per cell in registration order: min and
    /// median ns/op across rounds, batch and verdict.
    pub fn summary(&self) -> String {
        let mut out = format!("== {}\n", self.suite);
        for c in &self.calibrations {
            let mut ns: Vec<f64> = self
                .records
                .iter()
                .filter(|r| r.group == c.group && r.name == c.name)
                .map(|r| r.ns_per_op)
                .collect();
            ns.sort_by(|a, b| a.total_cmp(b));
            let (min, median) = if ns.is_empty() {
                (f64::NAN, f64::NAN)
            } else {
                (ns[0], ns[ns.len() / 2])
            };
            out.push_str(&format!(
                "{:>32}  min {:>14.1} ns/op  median {:>14.1} ns/op  (batch {}, {}, {} rounds)\n",
                c.path(),
                min,
                median,
                c.batch,
                c.calibration,
                ns.len()
            ));
        }
        out
    }
}
