---
title: "bensho: a small micro-benchmark harness that owns the boilerplate and leaves the data to the bench"
date: 2026-09-02
tags: [bensho, benchmark, csv, pandas, harness, rust, python]
status: implemented; the API, interleaving and CSV contract sections are superseded by specs/20260902_182536_bensho_suite_csv_sink_and_filter.md (suites, groups, one file per cell); the run loop, calibration and generic-versus-bespoke sections stand
related:
  - ~/github.com/hayeah/yudu/bench/embedbench/src/main.rs    # the reference implementation lifted from
  - ~/github.com/hayeah/yudu/bench/embed_analysis.py           # the analysis shape copied (calibrated-M flagging, per-cell stats)
  - ~/github.com/hayeah/yudu/bench/plots.py                    # the plot shape copied
  - src/lib.rs                                                 # the crate; code is ground truth
  - py/src/bensho/results.py                                   # the analysis side
---

# Purpose

Every micro-benchmark in the author's repos re-implements the same loop: warm
the subject up, pick a batch size that fits a time budget, time M operations
and divide, repeat for a few rounds, interleave subjects so machine drift is
shared, print CSV, then load the CSV in pandas. `bensho` is that loop as a
crate, with the data collection left entirely to the bench: a bench is a list
of cells (subject, mode, op closure) and a row type with whatever columns it
wants, and bensho supplies the rounds, the calibration, the shuffled
interleaving and the CSV writer.

The split it enforces is the one the yudu pipeline settled on: CSV production
is Rust, deterministic and reproducible from a seed; analysis is Python and
pandas and never needs to know the harness beyond the column names in this
document.

Publishable crate, not a vendored file. The Rust side has zero dependencies
(no criterion, no rayon, no serde, no csv crate); the Python side is a `uv`
project with pandas and matplotlib.

# The crate API

The code is the ground truth (`src/lib.rs`); this is the shape.

- `SplitMix(u64)` - splitmix64, the zero-dependency PRNG. `next_u64()`, `below(n)`.
- `schedule(seed, round, cells) -> Vec<usize>` - the cell order for one round; a pure function, see "Interleaving".
- `Options` - the run parameters, with `Options::parse(args)` and `Options::from_args()` as the CLI helper:
  - `rounds` (`--rounds`, default 5) - measured rounds after calibration.
  - `ops` (`--ops`, default 1,000,000) - the batch size ceiling M, in requested-op units.
  - `min_ops` (`--min-ops`, default 1,000) - the batch size floor; a cell so slow that the floor exceeds the budget runs at the floor and is flagged `Floor`.
  - `pilot_ops` (`--pilot`, default 1,024) - the pilot batch, capped at `ops`.
  - `budget` (`--budget-ms`, default 2,000) - the per-cell, per-round time target M is fitted to.
  - `seed` (`--seed`, default `0x5eed`, decimal or `0x` hex) - the shuffle seed.
  - `header` (`--no-header` clears it) - for concatenating runs.
  - `out` (`--out FILE`) - CSV destination; stdout when absent. The summary always goes to stderr.
  - `only` (`--only PATTERN`, repeatable) - substring filters on `subject/mode`; unmatched cells are dropped at registration, before the pilot.
  - `rest` - every argument bensho did not recognise, in order, for the bench's own flags. bensho never rejects an argument; the bench decides.
- `Row` - the user's columns. `fn columns() -> &'static [&'static str]` and `fn values(&self) -> Vec<String>`, one value per column, in order. `()` is the empty row. The `row!` macro declares a plain struct and its `Row` impl in one go; fields are any `Field` (the primitives, `String`, `&str`, `bool`, `char`, and `Option` of those, `None` printing as the empty cell so pandas reads NaN).
- `Sample<R>` - what an op closure returns: `ops` (the count actually performed, the divisor of ns/op), `elapsed` (`None` by default, meaning the harness's stopwatch around the call is the measurement; `Some` overrides it when the closure timed its own inner region), and `row: R`. `Sample::ops(n)` for `R = ()`, `Sample::with(n, row)` otherwise, `.timed(d)` to set `elapsed`.
- `Stopwatch` - an accumulator for the "time inside the engine" pattern: `time(|| call)` adds the call's wall time, `ns()` reads, `take()` reads and resets. The bench owns it and decides where it goes (usually into a `Row` column).
- `Bench<'a, R>` - `Bench::new(options)`, `.cell(subject, mode, op)` where `op: FnMut(usize) -> Sample<R> + 'a` receives the requested batch size, then `.run()` (CSV to `options.out` or stdout) or `.run_to(&mut impl Write)` (tests, in-process use). Both return a `Report`.
- `Report` - every `Record` emitted plus one `Calibrated` per cell, and `summary()`: per cell, min and median ns/op across rounds, batch size and calibration verdict, the text the harness prints to stderr.
- `Record` - bensho's own columns for one cell in one round, in CSV order (see "CSV contract"). `Calibration` is the enum `Full | Budget | Floor`; it prints as its identifier.
- `COLUMNS` - bensho's column names, in order, for anything that wants to check a CSV without parsing it.

The op closure captures its own state. Whatever must exist before the timed region (connections established, buffers allocated, probe sequences generated) is built once, when the closure is built, and reused across the pilot and every round. The closure's body is the whole timed region unless it reports its own `elapsed`.

# The run loop

- Registration: every `cell()` call appends a cell unless an `--only` filter excludes it.
- Calibration, in registration order, never shuffled: each cell runs one pilot batch of `min(pilot_ops, ops)` requested ops. That pilot is the warm-up. From its ns per requested op, `fit = budget / ns_per_request`:
  - `fit >= ops` - batch is `ops`, verdict `Full`.
  - `min_ops <= fit < ops` - batch is `fit`, verdict `Budget`.
  - `fit < min_ops` - batch is `min(min_ops, ops)`, verdict `Floor`; the round will exceed the budget, and the CSV says so.
- The batch is fixed before round 1 and never changes: every round of a cell is the same measurement, and the rate stays comparable across cells that were calibrated differently. That is why `batch`, `ops` and `calibration` are on every row rather than in a side file.
- Rounds `1..=rounds`: the round's order is `schedule(seed, round, cells)`. Each cell in that order runs one batch, and one record plus the cell's row is written and flushed immediately, so a killed run leaves a usable partial CSV.
- ns per op is `elapsed / ops` with `ops` the closure's reported count, not the requested batch. A closure that reports zero ops is a bench bug and aborts the run with a message naming the cell.
- After the last round the summary goes to stderr.

# Interleaving

The embedbench loop ran subjects in a fixed order within each round, so the
same subject always warmed the same successor and always sat in the first or
last slot. bensho shuffles instead.

- The cell set is every registered `(subject, mode)`. Every round is a permutation of all of them.
- The round's permutation is Fisher-Yates driven by `SplitMix(seed ^ round)`, with `round` 1-based and widened to `u64`. The generator is re-seeded per round from the seed and the round number alone, so:
  - the same seed and round give the same order on every machine and every run;
  - different rounds give different orders (splitmix64 hashes its state, so nearby seeds do not give nearby permutations);
  - changing `--rounds` leaves every earlier round's order untouched, since round k never consumes round j's randomness.
- Calibration is outside the shuffle: pilots run in registration order, once, before round 1.
- The CSV records `position` (0-based slot within the round), `cells` (the round's length) and `seed` on every row, so the execution order of the whole run is reconstructible from the CSV alone and drift can be checked empirically (see the Python side) rather than assumed away.
- A Williams or Latin-square schedule is out of scope. Those designs balance first-order carry-over only when the number of rounds is a multiple of the number of cells (k rounds for even k, 2k for odd), which a 30-cell, 5-round bench never satisfies; they also change wholesale when a cell is added, whereas a seeded shuffle with recorded positions stays valid for any cell count and lets the analysis measure the carry-over instead of trusting a design to cancel it.

# CSV contract

- One row per cell per round, bensho's columns first and in this order, then the user's columns in the order `Row::columns()` gives them. CSV keys are the Rust field names verbatim.
- `subject` - the implementation under test (a tier, a crate, a function).
- `mode` - the workload within the subject (hit/miss, a row kind, a size).
- `round` - 1-based measured round.
- `position` - 0-based slot within the round's shuffled order.
- `cells` - number of cells in the round.
- `seed` - the run's shuffle seed, as given.
- `batch` - the calibrated requested batch size (the closure's argument).
- `ops` - the op count the closure reported for this batch.
- `elapsed_ns` - integer nanoseconds for the batch (the harness stopwatch or the closure's own).
- `ns_per_op` - `elapsed_ns / ops`, three decimals.
- `calibration` - `Full`, `Budget` or `Floor`.
- `pilot_ns_per_op` - the pilot's ns per reported op, so the warm-up cost is a datum.
- `start_ms` - milliseconds from the start of round 1 to this batch's start, for wall-clock drift plots.
- Header rules: the header is written once, at the top, unless `--no-header`; a user column named like a bensho column aborts the run before the pilot; a row whose value count differs from its column count aborts at the first write. Values containing a comma, quote or newline are quoted RFC-4180 style; nothing else is quoted.
- Empty cells are the `Option::None` convention and read as NaN in pandas.
- Multiple runs concatenate by design: the same header, different seeds or machines, and a `source` column added by the loader.

# What is generic and what stays bespoke

The question the netstack rewrite asked: is a generic harness viable, or does
that bench need things a crate cannot give? The answer, per need:

- Count actual packets per op rather than bytes/1460 - the bench's job, served by a generic seam: the closure returns `Sample::ops(packets)` and bensho divides by it. bensho never knows what an op is.
- Reuse buffers across ops and across rounds - the bench's job; the closure captures its buffers and driver once. bensho's job is to never reconstruct the closure and to run the pilot on the same state.
- Time the engine's own calls separately from the harness's - the bench's job, with bensho's `Stopwatch` as the accumulator and a `Row` column (`engine_ns`) as the output; `engine_ns / ops` is one pandas line since `ops` is on the row. Whether the closure's `elapsed` should exclude the harness entirely is also the bench's call (`Sample::timed`).
- Report packets/s for small writes and payload Gbit/s only for bulk rows - the bench's job, and mostly the analysis's: the row carries `bytes` and a `kind` column, the CSV carries `ops` and `elapsed_ns`, and the report derives the unit per kind. bensho reports ns per op and nothing else.
- Sweep a parameter (N idle connections) into separate lines - the bench's job at registration: one cell per (tier, kind, N) with N encoded in `mode` and repeated as a numeric `Row` column so pandas can pivot on it. bensho's shuffle then interleaves the whole sweep.
- Per-row user columns - bensho's job: the `Row` trait, the `row!` macro, the header rules.
- Warm-up, calibration to a budget, batching, rounds, shuffled interleaving, seed, positions, CSV, the CLI flags for all of it - bensho's job.
- Sweeping active-flow position and socket count - the bench's job (more cells).
- Peak RSS - the bench's job today (a `Row` column from its own `peak_rss_bytes`), and honest only as a run-wide high-water mark; a per-cell RSS delta would need bensho to expose a per-cell hook, which it does not.

So the harness is viable. What stays bespoke is exactly the part that defines the bench: what an op is, what it costs inside versus outside the engine, and what unit a row is read in. Two things the netstack rewrite might want that bensho deliberately does not have: a per-batch setup hook outside the timed region (build the state once instead, or report `elapsed` yourself), and multi-threaded or process-isolated cells (out of scope; every cell runs on the calling thread).

# The Python pipeline

Under `py/`, a `uv` project laid out per the repo's `/python` skill: `src/bensho/`, tests beside sources, `typer` for the CLI, `ruff` and `pyright` via `Makefile.py`.

- `results.py` - `Results`, the whole read-out as one class over one DataFrame: `Results.load(paths)` concatenates any number of bensho CSVs and adds `source`; `census()` (rows present versus `cells x rounds` expected, per source); `cells()` (per subject/mode: rounds, min, median, mean, std, CV%, batch, ops, calibration, a `calibrated` flag - the yudu convention, a down-calibrated cell has fewer ops behind its rate and is flagged, never silently averaged in); `anomalies()` (calibrated cells and CV above 5%); `round_drift()` (median of ns/op normalised to the cell's median, per round - does the machine slow over the run); `position_drift()` (the same normalised value against slot fraction - does the slot a cell drew matter; slope from first to last slot in percent, and the correlation); `figure()` (a dot-and-range chart, median with min-max whiskers, one colour per subject in fixed first-seen order, log y); `dump(dir)` writes every table as TSV named after its method.
- `results_notebook.py` - the percent-format notebook over `Results`, per the skill's notebook convention; `Makefile.py` has the jupytext + papermill task.
- `report.py` - the CLI: `bensho-report CSV... [--plot PATH] [--dump DIR]` prints the tables and saves the figure. Also runnable without installing: `uv run --with pandas,matplotlib,typer python py/src/bensho/report.py results.csv`.
- The contract with the CSV is the column list above and nothing else: user columns pass through `Results.df` untouched and appear in `user_columns`.
- No notebook existed in yudu's `bench/` (only `.py` and rendered `plots/*.png`), so the notebook here is new, written to the skill's convention rather than copied.

# Worked example

`examples/toy.rs`, the bench the tests and README use. Two subjects (`vec_sum`
and `iter_sum`) over two modes (`small`, `large`), a row with a `bytes` column:

```rust
bensho::row! { struct ToyRow { bytes: u64 } }

let opts = bensho::Options::from_args();
let small: Vec<u64> = (0..1_000).collect();
let large: Vec<u64> = (0..1_000_000).collect();
let mut bench = bensho::Bench::<ToyRow>::new(opts);
for (mode, data) in [("small", &small), ("large", &large)] {
    bench.cell("vec_sum", mode, move |m| {
        let mut acc = 0u64;
        for _ in 0..m {
            acc = acc.wrapping_add(black_box(data).iter().sum::<u64>());
        }
        black_box(acc);
        Sample::with(m as u64, ToyRow { bytes: (data.len() * 8) as u64 })
    });
}
bench.run().unwrap();
```

`cargo run --release --example toy -- --rounds 3 --out toy.csv` then
`cd py && uv run bensho-report ../toy.csv --plot toy.png`.

# Non-goals

- Statistical machinery beyond min/median/CV in the report; the CSV is the product and pandas is the tool.
- Wall-clock isolation: no CPU pinning, no priority changes, no process-per-cell.
- Multi-threaded cells.
- Balanced (Williams) schedules, for the reason above.
- serde integration: the `Row` trait plus `row!` cost twenty lines; naming columns from a `Serialize` impl without the `csv` crate costs a bespoke serializer of two hundred, for no gain a bench author would notice.
- A criterion-style output format, comparison against a saved baseline, or a plotting story in Rust.
