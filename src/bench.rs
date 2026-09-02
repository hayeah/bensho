//! The run loop: pilots, calibration, shuffled rounds, records.

use std::fmt;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::csv::CSVWriter;
use crate::{schedule, Options, Row};

/// bensho's own columns, in CSV order. `Record`'s field names verbatim.
pub const COLUMNS: &[&str] = &[
    "subject",
    "mode",
    "round",
    "position",
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

/// What an op closure returns: the count performed, optionally its own
/// timing, and the bench's row.
#[derive(Clone, Debug)]
pub struct Sample<R = ()> {
    /// Ops actually performed; the divisor of ns/op.
    pub ops: u64,
    /// `None`: the harness stopwatch around the call is the measurement.
    /// `Some`: the closure timed its own region.
    pub elapsed: Option<Duration>,
    pub row: R,
}

impl Sample<()> {
    pub fn ops(ops: u64) -> Sample<()> {
        Sample {
            ops,
            elapsed: None,
            row: (),
        }
    }
}

impl<R> Sample<R> {
    pub fn with(ops: u64, row: R) -> Sample<R> {
        Sample {
            ops,
            elapsed: None,
            row,
        }
    }

    pub fn timed(mut self, elapsed: Duration) -> Sample<R> {
        self.elapsed = Some(elapsed);
        self
    }
}

/// An accumulator for "time inside the engine": wrap each engine call in
/// `time`, read `ns` at the end of the batch, put it in a row column.
#[derive(Clone, Debug, Default)]
pub struct Stopwatch {
    ns: u128,
}

impl Stopwatch {
    pub fn new() -> Stopwatch {
        Stopwatch::default()
    }

    pub fn time<T>(&mut self, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let out = f();
        self.ns += start.elapsed().as_nanos();
        out
    }

    pub fn ns(&self) -> u128 {
        self.ns
    }

    pub fn reset(&mut self) {
        self.ns = 0;
    }

    /// Read and reset.
    pub fn take(&mut self) -> u128 {
        std::mem::take(&mut self.ns)
    }
}

/// bensho's columns for one cell in one round.
#[derive(Clone, Debug)]
pub struct Record {
    pub subject: String,
    pub mode: String,
    pub round: u32,
    pub position: usize,
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
    /// The values in `COLUMNS` order.
    pub fn values(&self) -> Vec<String> {
        vec![
            self.subject.clone(),
            self.mode.clone(),
            self.round.to_string(),
            self.position.to_string(),
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
    pub subject: String,
    pub mode: String,
    pub batch: usize,
    pub calibration: Calibration,
    pub pilot_ns_per_op: f64,
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

/// Everything a run produced, for tests and in-process consumers.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub records: Vec<Record>,
    pub calibrations: Vec<Calibrated>,
}

impl Report {
    /// Per cell: min and median ns/op across rounds, batch and verdict.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        for c in &self.calibrations {
            let mut ns: Vec<f64> = self
                .records
                .iter()
                .filter(|r| r.subject == c.subject && r.mode == c.mode)
                .map(|r| r.ns_per_op)
                .collect();
            ns.sort_by(|a, b| a.total_cmp(b));
            let (min, median) = if ns.is_empty() {
                (f64::NAN, f64::NAN)
            } else {
                (ns[0], ns[ns.len() / 2])
            };
            out.push_str(&format!(
                "{:>24}  min {:>14.1} ns/op  median {:>14.1} ns/op  (batch {}, {}, {} rounds)\n",
                format!("{}/{}", c.subject, c.mode),
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

type Op<'a, R> = Box<dyn FnMut(usize) -> Sample<R> + 'a>;

struct Cell<'a, R> {
    subject: String,
    mode: String,
    op: Op<'a, R>,
    batch: usize,
    calibration: Calibration,
    pilot_ns_per_op: f64,
}

/// The harness. Register cells, then `run`.
pub struct Bench<'a, R: Row = ()> {
    options: Options,
    cells: Vec<Cell<'a, R>>,
}

impl<'a, R: Row> Bench<'a, R> {
    pub fn new(options: Options) -> Bench<'a, R> {
        Bench {
            options,
            cells: Vec::new(),
        }
    }

    pub fn options(&self) -> &Options {
        &self.options
    }

    /// Register one `(subject, mode)` cell. `op` receives the requested batch
    /// size and returns the count it performed; its body is the timed region.
    /// Cells excluded by `--only` are dropped here, before the pilot.
    pub fn cell(
        &mut self,
        subject: impl Into<String>,
        mode: impl Into<String>,
        op: impl FnMut(usize) -> Sample<R> + 'a,
    ) -> &mut Self {
        let (subject, mode) = (subject.into(), mode.into());
        if self.options.keeps(&subject, &mode) {
            self.cells.push(Cell {
                subject,
                mode,
                op: Box::new(op),
                batch: 0,
                calibration: Calibration::Full,
                pilot_ns_per_op: 0.0,
            });
        }
        self
    }

    /// Calibrate, run the rounds, write CSV to `options.out` or stdout, print
    /// the summary to stderr.
    pub fn run(self) -> io::Result<Report> {
        let report = match self.options.out.clone() {
            Some(path) => {
                let mut file = io::BufWriter::new(std::fs::File::create(path)?);
                self.run_to(&mut file)?
            }
            None => {
                let stdout = io::stdout();
                let mut lock = stdout.lock();
                self.run_to(&mut lock)?
            }
        };
        eprint!("{}", report.summary());
        Ok(report)
    }

    /// Calibrate, run the rounds, write CSV to `out`. No summary is printed.
    pub fn run_to(mut self, out: &mut dyn Write) -> io::Result<Report> {
        let mut writer = CSVWriter::new(out, R::columns(), self.options.header)?;
        let mut report = Report::default();

        for cell in &mut self.cells {
            let pilot = self.options.pilot_ops.min(self.options.ops);
            let (sample, elapsed) = measure(cell, pilot)?;
            let per_request = elapsed.as_nanos() as f64 / pilot as f64;
            let (batch, calibration) = calibrate(per_request, &self.options);
            cell.batch = batch;
            cell.calibration = calibration;
            cell.pilot_ns_per_op = elapsed.as_nanos() as f64 / sample.ops as f64;
            report.calibrations.push(Calibrated {
                subject: cell.subject.clone(),
                mode: cell.mode.clone(),
                batch,
                calibration,
                pilot_ns_per_op: cell.pilot_ns_per_op,
            });
        }

        let cells = self.cells.len();
        let seed = self.options.seed;
        let run_start = Instant::now();
        for round in 1..=self.options.rounds {
            for (position, index) in schedule(seed, round, cells).into_iter().enumerate() {
                let cell = &mut self.cells[index];
                let start_ms = run_start.elapsed().as_millis();
                let (sample, elapsed) = measure(cell, cell.batch)?;
                let record = Record {
                    subject: cell.subject.clone(),
                    mode: cell.mode.clone(),
                    round,
                    position,
                    cells,
                    seed,
                    batch: cell.batch,
                    ops: sample.ops,
                    elapsed_ns: elapsed.as_nanos(),
                    ns_per_op: elapsed.as_nanos() as f64 / sample.ops as f64,
                    calibration: cell.calibration,
                    pilot_ns_per_op: cell.pilot_ns_per_op,
                    start_ms,
                };
                writer.record(&record, &sample.row)?;
                report.records.push(record);
            }
        }
        Ok(report)
    }
}

/// Run one batch under the harness stopwatch; the closure's own `elapsed`
/// wins when it reports one.
fn measure<R>(cell: &mut Cell<'_, R>, batch: usize) -> io::Result<(Sample<R>, Duration)> {
    let start = Instant::now();
    let sample = (cell.op)(batch);
    let harness = start.elapsed();
    if sample.ops == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "cell {}/{} reported zero ops for a batch of {batch}",
                cell.subject, cell.mode
            ),
        ));
    }
    let elapsed = sample.elapsed.unwrap_or(harness);
    Ok((sample, elapsed))
}
