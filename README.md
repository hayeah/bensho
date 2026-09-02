# bensho

A small micro-benchmark harness for Rust. It owns the loop: warm-up, calibration
of the batch size to a time budget, batching, rounds, seeded interleaving of
cells within a round, and CSV output. The bench owns the data: what an op is,
how many it did, and whatever columns a row carries. Analysis is pandas, in
`py/`, and never needs to know the harness beyond the column names.

Zero dependencies on the Rust side. Code is the ground truth; this file is an
index of pointers.

- [specs/20260902_180344_bensho_micro_benchmark_harness.md](specs/20260902_180344_bensho_micro_benchmark_harness.md) - purpose, the API shape, the CSV contract, the interleaving design, calibration rules, what is generic versus bespoke, non-goals.
- [src/lib.rs](src/lib.rs) - the crate root and its exports; the module docs say what each piece is for.
- [src/bench.rs](src/bench.rs) - `Bench`, `Sample`, `Record`, `Calibration`, `Stopwatch`, the run loop, `COLUMNS`.
- [src/rng.rs](src/rng.rs) - splitmix64 and `schedule(seed, round, cells)`, the per-round permutation.
- [src/options.rs](src/options.rs) - `Options`, the CLI flags, `USAGE`.
- [src/row.rs](src/row.rs) - the `Row` trait, `Field`, the `row!` macro.
- [examples/toy.rs](examples/toy.rs) - the worked example: two subjects, two modes, one user column.
- [tests/harness.rs](tests/harness.rs) - the spec's claims checked: shuffle determinism, the calibration rule, the CSV shape.
- [py/src/bensho/results.py](py/src/bensho/results.py) - `Results`: load CSVs, per-cell statistics, calibrated-cell flags, drift by round, slot and predecessor, the figure.
- [py/src/bensho/report.py](py/src/bensho/report.py) - `bensho-report CSV... [--plot PATH] [--dump DIR]`.
- [py/src/bensho/results_notebook.py](py/src/bensho/results_notebook.py) - the percent-format notebook over `Results`.
- [worklogs/](worklogs/) - dated records of what was built and measured.

## Running

```
cargo run --release --example toy -- --rounds 3 --out toy.csv   # flags: USAGE in src/options.rs

cd py && uv run bensho-report ../toy.csv --plot toy.png
uvx --from ./py bensho-report toy.csv               # without entering py/

cargo test
cargo clippy --all-targets -- -D warnings
cd py && uv run pymake all                          # ruff, pyright, pytest
cd py && uv run pymake notebook --csv ../toy.csv    # jupytext + papermill into py/output/
```

## The shape of a bench

```rust
use bensho::{Bench, Options, Sample, Stopwatch};

bensho::row! { struct PacketRow { bytes: u64, engine_ns: u128 } }

let mut bench = Bench::<PacketRow>::new(Options::from_args());
for n in [100, 400, 1000] {
    let mut engine = Engine::with_idle_connections(n);   // built once, reused every round
    let mut clock = Stopwatch::new();
    bench.cell("lwip", format!("small-write/idle{n}"), move |m| {
        let mut packets = 0;
        for _ in 0..m {
            packets += clock.time(|| engine.write_small());
        }
        Sample::with(packets, PacketRow { bytes: m as u64 * 100, engine_ns: clock.take() })
    });
}
bench.run().unwrap();
```

The closure receives the requested batch size and reports the count it
performed; ns/op divides by the count. Everything the closure captures is built
once and survives the pilot and every round.
