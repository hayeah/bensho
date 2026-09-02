//! bensho: a small micro-benchmark harness. It owns warm-up, calibration of
//! the batch size to a time budget, batching, rounds, seeded interleaving of
//! cells within a round, and CSV output; the bench owns what an op is and
//! what columns a row carries.
//!
//! A bench is a list of cells, each a `(subject, mode)` name pair and an op
//! closure that performs a requested number of ops and reports how many it
//! did (`Sample`). Rows carry the bench's own columns through the `Row`
//! trait (`row!` writes the impl). See `examples/toy.rs`.
//!
//! The CSV contract and the design are in
//! `specs/20260902_180344_bensho_micro_benchmark_harness.md`; the columns
//! are `COLUMNS` followed by `Row::columns()`.

mod bench;
mod csv;
mod options;
mod rng;
mod row;

pub use bench::{
    calibrate, Bench, Calibrated, Calibration, Record, Report, Sample, Stopwatch, COLUMNS,
};
pub use csv::quote;
pub use options::{Options, USAGE};
pub use rng::{schedule, SplitMix};
pub use row::{Field, Row};
