# bensho

A small micro-benchmark harness for Rust. It owns the loop: warm-up,
calibration of the batch size to a time budget, batching, rounds, seeded
interleaving at two levels, one CSV file per cell. The bench owns the data:
what an op is, what state a cell needs, how many ops it did, and whatever
columns a row carries. Analysis is pandas, in `py/`, and never needs to know
the harness beyond the column names.

Zero dependencies on the Rust side. Code is the ground truth; this file is an
index of pointers.

- [specs/20260902_182536_bensho_suite_csv_sink_and_filter.md](specs/20260902_182536_bensho_suite_csv_sink_and_filter.md) - the current design: program, suite, group, cell; state built per visit; the nested shuffle; the files; the filters; the CSV contract; the Python side.
- [specs/20260902_180344_bensho_micro_benchmark_harness.md](specs/20260902_180344_bensho_micro_benchmark_harness.md) - the original: purpose, the run loop, calibration rules, what is generic versus bespoke. Its API and CSV sections are superseded by the spec above.
- [src/lib.rs](src/lib.rs) - the crate root and its exports; the module docs say what each piece is for.
- [src/harness.rs](src/harness.rs) - `Harness`: the options, the output directory, `suite()`.
- [src/suite.rs](src/suite.rs) - `Suite`, `Group`, the erased per-visit state, calibration and the shuffled rounds, `check_name`.
- [src/record.rs](src/record.rs) - `Record`, `Calibrated`, `Report`, `Calibration`, `calibrate`, `COLUMNS`, `cell_path`.
- [src/sample.rs](src/sample.rs) - `Sample`, `Stopwatch`.
- [src/rng.rs](src/rng.rs) - splitmix64, `schedule` (groups per round) and `schedule_in_group` (cells per group).
- [src/options.rs](src/options.rs) - `Options`, the CLI flags, `USAGE`, the two filter questions.
- [src/row.rs](src/row.rs) - the `Row` trait, `Field`, the `row!` macro.
- [src/csv.rs](src/csv.rs) - one file per cell, header on create, open-append-close per row.
- [examples/toy.rs](examples/toy.rs) - the worked example: two groups whose state is a slice built per visit, two cells each, one singleton.
- [tests/harness.rs](tests/harness.rs) - the spec's claims checked: the two schedules, one state alive at a time, setup counts, files and headers, filters, `--list`, calibration.
- [py/src/bensho/results.py](py/src/bensho/results.py) - `Results`: load files or directories, census per file, per-cell statistics, calibrated flags, drift and carry-over at the round, group and cell levels, the figure.
- [py/src/bensho/report.py](py/src/bensho/report.py) - `bensho-report PATH... [--plot PATH] [--dump DIR] [--color-by COL]`.
- [py/src/bensho/results_notebook.py](py/src/bensho/results_notebook.py) - the percent-format notebook over `Results`.
- [worklogs/](worklogs/) - dated records of what was built and measured.

## Running

```
cargo run --release --example toy -- --rounds 3 --out out/   # flags: USAGE in src/options.rs
cargo run --release --example toy -- --list
cargo run --release --example toy -- --only toy/small/ --out out/   # one group; other files untouched

cd py && uv run bensho-report ../out/toy --plot toy.png --color-by group
uvx --from ./py bensho-report out/                  # every suite under out/, without entering py/

cargo test
cargo clippy --all-targets -- -D warnings
cd py && uv run pymake all                          # ruff, pyright, pytest
cd py && BENSHO_RESULTS=../out uv run pymake notebook   # jupytext + papermill into py/output/
```

## The shape of a bench

```rust
use bensho::{Group, Harness, Sample, Stopwatch, Suite};

bensho::row! { struct PacketRow { idle: u32, bytes: u64, engine_ns: u128 } }

struct Bed { engine: Engine, clock: Stopwatch }

fn main() -> std::io::Result<()> {
    let harness = Harness::from_args();
    harness.suite("packets", |s: &mut Suite<PacketRow>| {
        for idle in [100u32, 1000] {
            // Built each time a round reaches this group, dropped after its
            // last cell: one engine alive at a time, every round on a fresh one.
            let setup = move || Bed { engine: Engine::with_idle_connections(idle), clock: Stopwatch::new() };
            s.group(format!("lwip/idle{idle}"), setup, |g: &mut Group<PacketRow, Bed>| {
                g.cell("small-write", move |bed, m| {
                    let mut packets = 0;
                    for _ in 0..m {
                        packets += bed.clock.time(|| bed.engine.write_small());
                    }
                    Sample::with(packets, PacketRow { idle, bytes: m as u64 * 100, engine_ns: bed.clock.take() })
                });
            });
        }
    })?;
    Ok(())
}
```

Files land at `<out>/packets/lwip/idle100/small-write.csv` and so on, one row
per round, bensho's columns first (`suite`, `group`, `name`, `round`,
`position`, `group_position`, ...) then `data.idle`, `data.bytes`,
`data.engine_ns`. The closure receives the group's state and the requested
batch size and reports the count it performed; ns/op divides by the count.
Groups are shuffled per round, cells within a group too, and both orders are
on every row.
