//! bensho: a small micro-benchmark harness. It owns warm-up, calibration of
//! the batch size to a time budget, batching, rounds, seeded interleaving,
//! and the CSV files; the bench owns what an op is, what state a cell needs
//! and what columns a row carries.
//!
//! A program is a `Harness`. It runs suites one after another; a suite is a
//! directory and one row type; inside it, a group is a state built each time
//! the round reaches it and the cells that use it; a cell is a file with one
//! row per round. Rows carry the bench's own columns through the `Row` trait
//! (`row!` writes the impl), namespaced under `data.`. See `examples/toy.rs`.
//!
//! The design and the CSV contract are in
//! `specs/20260902_182536_bensho_suite_csv_sink_and_filter.md`; the columns
//! are `COLUMNS` followed by `data.<field>` for each of `Row::columns()`.

mod csv;
mod harness;
mod options;
mod record;
mod rng;
mod row;
mod sample;
mod suite;

pub use csv::quote;
pub use harness::Harness;
pub use options::{Options, USAGE};
pub use record::{calibrate, cell_path, Calibrated, Calibration, Record, Report, COLUMNS};
pub use rng::{schedule, schedule_in_group, SplitMix};
pub use row::{Field, Row};
pub use sample::{Sample, Stopwatch};
pub use suite::{check_name, Group, Suite};
