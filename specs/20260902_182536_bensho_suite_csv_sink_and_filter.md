---
title: "bensho: a suite is a directory of one row type, a group is a state built per visit, a cell is a CSV file; user columns under data."
date: 2026-09-02
tags: [bensho, benchmark, csv, suite, group, filter, cli, rust]
status: proposed; amended the same day with the group level before implementation. Supersedes the API and CSV contract sections of the first spec; its run loop and calibration sections stand, its interleaving section is extended by "Groups".
related:
  - specs/20260902_180344_bensho_micro_benchmark_harness.md      # the harness this amends
  - ~/github.com/hayeah/yudu/bench/embedbench/src/main.rs        # the original: the bench prints its own CSV rows, the caller owns the file
  - ~/github.com/hayeah/yudu/bench/embed_bench.py                 # run_matrix: one file per matrix
  - ~/github.com/hayeah/yudu/bench/matrix.py                      # one CSV per matrix function (results, sweep_results, quick_results)
  - src/harness.rs, src/suite.rs, src/options.rs, src/csv.rs, src/rng.rs, src/row.rs   # what changes
---

# The complaints

- Organising many benchmarks in one binary is awkward. `Bench<R>` is one row type, so a program with two row shapes needs two `Bench` values, each with its own `Options` copy, and `run()` writes wherever `options.out` points: both write the same `--out` file (the second truncates the first) or both dump to stdout with two headers. Nothing has a name above the cell: not in the API, not in the filter, not in the summary.
- The CSV destination is hidden inside `Options`. The original artefact kept it outside the harness: embedbench printed rows, the file belonged to whoever ran the binary, and yudu's drivers wrote one CSV per matrix. bensho's `run()` took that choice away and gave back one flag.
- There is no way to run a subset by group. `--only` matches `subject/mode` substrings, and since a bench builds its state before registering cells, a filter that only drops cells at registration cannot skip the expensive setup of a group nobody asked for.
- The two-level `(subject, mode)` cell identity is arbitrary. One-dimensional benches invent a mode; three-dimensional ones overflow it (`small-write/idle100`) and repeat the third dimension as a column so pandas can pivot. Dimensions past the second are columns already, so all of them should be.
- User columns and bensho columns share one namespace, so a bench cannot have a column called `ops` or `bytes` if bensho ever wants the name.
- A cell's state lives in its closure for the whole run. Every round measures state that earlier rounds already touched, nothing bounds how many states are alive at once (one per cell), and a bench that wants a fresh state per round has to rebuild inside the timed region or ask for a setup hook the first spec refused.

# The hierarchy

```
<out_dir>/                 the program: one process, one --out DIR
  <suite>/                 a suite: one row type, one seeded shuffle, one header shape
    <group>/               a group: one state, built when the group is visited, dropped after
      <cell>.csv           a cell: one file, one row per round
        data.<field>       the row: the suite's typed columns, namespaced
```

- The program is a process. It owns the parsed `Options` and the output directory. Nothing else lives at this level.
- A suite is a directory and a row type. Every cell in it returns the same `Row`, so every file in the directory has the same header, and the directory is a dataset: `ls` lists the cells, the report loads the directory. Groups of a suite are interleaved by the seeded shuffle, and cells within a group by a second one; suites run one after another, in call order, whole. Cells that must share machine drift belong in one suite.
- A group is a state and the cells that use it. The state is built by the group's `setup` closure each time the schedule reaches the group and dropped when the group's last cell of that round is done (see "Groups: state per visit"). A cell registered directly on the suite is a group of one whose state is `()`; it has no group directory and no group name.
- A cell is a file. Its name is a relative path (`hit`, `small-write/idle100`); bensho appends `.csv`, so slashes in a name become subdirectories. A grouped cell's file is `<group>/<cell>.csv`. The cell's path within the suite (`<group>/<cell>`, or `<cell>` for a singleton) heads the summary and is the filter target; `group` and `name` are two columns in every one of its rows.
- A row is a struct declared with `row!`, as before. Its fields become columns `data.<field>` in declaration order. bensho's own columns keep plain names. The two namespaces cannot collide, so any field name is allowed.

Names, suite, group and cell alike, must be relative, contain no `..` and no empty components, and not end in `.csv` (there is one spelling of a file). A suite or group name may contain slashes (`netstack/packets`). The same suite name twice in one process is an error; the same cell path twice in one suite is an error (two groups may each have a cell `hit`, since the paths differ; a group `a` with a cell `b` and a singleton cell `a/b` collide).

# Files and partial reruns

- When a suite starts, after filtering and before any pilot, bensho creates `<out_dir>/<suite>/` and, for each cell that will run, creates (truncating) its file with the header, subdirectories included. Cells that are not going to run are not touched.
- Each record is appended to its cell's file as it is produced, open-append-close, so no handles stay open and a killed run leaves every file valid up to its last row.
- The consequence is that reruns compose on disk. `--only packets/chd/` rewrites the files under `packets/chd/` and leaves the other files from the previous run in place. Whether rows in a directory came from one run or several is visible in every row: `seed`, `cells`, `start_ms`. The report's census reports it per file.
- Execution order is reconstructible across files: every row carries `round`, `position`, `group_position` and `cells`, so sorting a suite's rows by `(round, position)` recovers the sequence and grouping them by `(round, group)` recovers which cells shared a state.

# The API

The shape; the code will be ground truth once written.

- `Harness` - the program-level object. `Harness::from_args()` (parse argv, exit with usage on error) and `Harness::new(options)`. `options()` borrows the run parameters.
- `Harness::suite<'a, R: Row>(&self, name: &str, build: impl FnOnce(&mut Suite<'a, R>)) -> io::Result<Option<Report>>` - the one way cells run. Validates `name`. When `--suite` admits it: build a `Suite` with a copy of the harness's options, call `build` to register groups and cells (cells `--only`/`--skip` exclude are dropped at registration, and a group left with no cells is dropped with them, so its `setup` never runs), create the directory and the files as above, calibrate every group in registration order, run the rounds, append one row per record, print the summary to stderr under a `== <suite>` heading, return `Some(report)`. When `--suite` excludes it: `build` is not called, nothing on disk is touched, one line `-- <suite>: skipped` goes to stderr, `Ok(None)` is returned. A suite with no cells after filtering creates nothing and says so. Under `--list` every call prints its cells and returns `Ok(None)`; the process ends when `main` does.
- `Suite<'a, R>` - `cell(name, op)` where `op: FnMut(usize) -> Sample<R> + 'a` registers a singleton; `group(name, setup, build)` where `setup: FnMut() -> S + 'a` and `build: FnOnce(&mut Group<'a, R, S>)` registers a group; `name()`, `options()`. `options_mut()` lets the build closure change the run parameters for this suite alone (a slow suite wants a longer budget; a sweep wants fewer rounds); the CLI values are what it starts from. No constructor and no `run`; only `Harness::suite` makes one. The state type `S` is erased inside the suite (each group is boxed behind a trait), so one suite mixes groups of different state types, and the row type `R` is the suite's.
- `Group<'a, R, S>` - `cell(name, op)` where `op: FnMut(&mut S, usize) -> Sample<R> + 'a`, and `name()`. Only `Suite::group` makes one.
- `Options` - `rounds`, `ops`, `min_ops`, `pilot_ops`, `budget`, `seed` and `rest` as before. `out_dir: PathBuf` (`--out DIR`, default `.`). `suite: Vec<String>` (`--suite SUBSTR`, repeatable, OR'd; a suite runs when its name contains one of them, or when there are none). `only` and `skip: Vec<String>` (`--only SUBSTR`, `--skip SUBSTR`, repeatable; a cell runs when `<suite>/<path>` contains some `only` pattern, or there are none, and contains no `skip` pattern). `list: bool` (`--list`). `header` and `out` are gone. `enters(suite)` and `keeps(suite, path)` are the two filter questions.
- `Row`, `row!`, `Field` - unchanged. `columns()` still returns the bare field names; the writer adds the `data.` prefix.
- `Sample`, `Stopwatch`, `Calibration`, `Calibrated`, `SplitMix`, `calibrate`, `quote` - unchanged. `schedule(seed, round, groups)` keeps its signature and now orders groups; `schedule_in_group(seed, round, group_index, cells)` orders one group's cells.
- `Record` - `subject` and `mode` replaced by `suite`, `group` and `name`; `group_position` added next to `position`; `path()` gives `<group>/<name>` or `<name>`. `COLUMNS` updated to match. `Calibrated` carries `group` and `name`. `Report` gains `suite: String`; `summary()` prints the heading and one line per cell, in registration order, named by path.
- The CSV writer is internal. No public sink type, no `--no-header`, no stdout output. Concatenation across runs, binaries or machines is the loader's job.

# Groups: state per visit

A group is the unit of state. `Suite::group(name, setup, build)`: `setup` constructs the state, `build` registers the cells that use it, each taking `&mut S` and the requested batch size. Nothing is built at registration; `setup` runs when the schedule reaches the group.

- Schedule. Per round, the groups are shuffled with `SplitMix(seed ^ round)`, the same permutation `schedule(seed, round, n)` the first spec used for cells. Within each group, its cells are shuffled with a second stream, `SplitMix(seed ^ round ^ ((group_index + 1) << 32))` where `group_index` is the group's registration index (not its slot in the round) and the `+ 1` keeps the first group's stream distinct from the group-level one. Both are pure functions of their arguments, so the same seed and round give the same orders everywhere, adding a group at the end changes neither the earlier groups' inner orders nor any earlier round, and changing `--rounds` leaves earlier rounds untouched. Singleton cells participate as groups of one.
- State lifetime. When a round reaches a group, `setup` is called and the state built; the group's cells run in their inner order, each borrowing the state mutably; the state is dropped after the group's last cell. Exactly one group's state is alive at a time, and none between groups. Calibration before round one does the same per group, in registration order: build, pilot and calibrate each of its cells, drop. Setup and drop are outside every timed region; `start_ms` is taken after setup.
- What that measures. Every round measures a freshly built state, so round-to-round variance is the machine's, not the state's history. Within a round, a group's cells share what earlier cells left behind (allocator state, caches, connection tables, whatever the state holds), by design: cells of one group are the cells that must see the same subject state, and the within-group shuffle plus the recorded `position` let the analysis measure that carry-over (`carryover(level="cell")`, `position_drift(level="cell")`) instead of assuming it away. The between-group shuffle plus `group_position` do the same one level up.
- Rationale. Interleaving granularity moves to the group level for subject adjacency: a group is typically one subject's cells, and the round order is then which subject follows which. Memory is bounded to one state rather than one per cell, which is what lets a bench sweep large states (a million idle connections at three sizes is three states, one at a time). Rebuild cost is untimed and, for the states benches actually build, small next to a two-second budget times the group's cell count. This replaces both the per-cell setup hook the first spec refused and any `Batch` handle: setup is the group's `setup`, and `Sample::timed` remains the way a cell reports its own timed sub-region.
- A cell that wants state that survives every round (a warmed cache measured warm) captures it in its closure as before; the closure outlives the run, `setup` is per visit. Both shapes coexist in one suite.

# The filter

Every pattern is a case-sensitive substring; there is no grammar.

- `--suite SUBSTR` selects suites by name. This is the pre-skip: an unselected suite never runs its build closure, so nothing in it is constructed. `--suite packets` and `--suite netstack/` both read as expected.
- `--only SUBSTR` and `--skip SUBSTR` select cells by their full path `<suite>/<group>/<cell>` (or `<suite>/<cell>` for a singleton). `--only packets/chd/` keeps one group, `--only /miss` keeps one workload everywhere, `--skip include_dir` drops a slow subject everywhere. `--skip` is applied after `--only`. A group whose cells are all excluded is not visited and its state never built.
- `--list` prints `<suite>/<group>/<cell>` for every cell that would run, one per line. Build closures run (they are where cells come from), `setup` closures do not, no pilot runs and nothing is created.

# CSV contract

Replaces the first spec's column list. bensho's columns first, in this order, then `data.<field>` for each `Row` field in declaration order. Every file in a suite has the same header.

- `suite` - the suite's name, so a directory holding several suites still reconstructs each suite's rounds.
- `group` - the group's name, or the empty string for a singleton cell (the loader reads it back as the empty string, not NaN).
- `name` - the cell's name as registered. `<group>/<name>` is the file's path within the suite directory, without `.csv`; for a singleton, `<name>` is.
- `round` - 1-based measured round.
- `position` - 0-based slot within the round's flattened order (groups in their order, each group's cells in theirs).
- `group_position` - 0-based slot of the cell's group within the round's group order.
- `cells` - number of cells in the round.
- `seed` - the run's shuffle seed, as given.
- `batch` - the calibrated requested batch size.
- `ops` - the op count the closure reported for this batch.
- `elapsed_ns` - integer nanoseconds for the batch.
- `ns_per_op` - `elapsed_ns / ops`, three decimals.
- `calibration` - `Full`, `Budget` or `Floor`.
- `pilot_ns_per_op` - the pilot's ns per reported op.
- `start_ms` - milliseconds from the start of round 1 to this batch's start, after the group's setup.
- `data.<field>` - the suite's columns. `Option::None` is the empty cell.

The header is the first line of every file. A row whose value count differs from its column count aborts at the first write. Quoting is RFC-4180, applied only where a value needs it. There is no collision rule any more: `data.` is bensho's to write and a field named `ops` becomes `data.ops`.

# Worked example

The toy bench becomes a one-suite program with two groups and one singleton. A group's `setup` builds the slice the group's cells sum; the op closure receives the state and the requested batch size `m`, does the work, and returns a `Sample` with the count it performed and one typed row. Every cell of a suite returns the same row type, which is what gives the directory one header and is checked by the compiler.

```rust
use std::hint::black_box;
use bensho::{Group, Harness, Sample, Suite};

bensho::row! {
    /// Which summation, over how many bytes. `subject` repeats the cell
    /// name so the report can colour by it.
    struct ToyRow { subject: &'static str, bytes: u64 }
}

fn main() -> std::io::Result<()> {
    let harness = Harness::from_args();
    harness.suite("toy", |s: &mut Suite<ToyRow>| {
        for (size, len) in [("small", 1_000usize), ("large", 1_000_000)] {
            // The state: built when the round reaches this group, dropped after
            // its last cell. Never two of them alive at once.
            let setup = move || (0..len as u64).collect::<Vec<u64>>();
            s.group(size, setup, |g: &mut Group<ToyRow, Vec<u64>>| {
                g.cell("vec_sum", |data, m| {
                    let mut acc = 0u64;
                    for _ in 0..m {
                        acc = acc.wrapping_add(black_box(&*data).iter().sum::<u64>());
                    }
                    black_box(acc);
                    Sample::with(m as u64, ToyRow { subject: "vec_sum", bytes: (data.len() * 8) as u64 })
                });
                g.cell("fold_sum", |data, m| {
                    let mut acc = 0u64;
                    for _ in 0..m {
                        acc = acc.wrapping_add(
                            black_box(&*data).iter().fold(0u64, |a, &x| a.wrapping_add(x)),
                        );
                    }
                    black_box(acc);
                    Sample::with(m as u64, ToyRow { subject: "fold_sum", bytes: (data.len() * 8) as u64 })
                });
            });
        }
        // A singleton: a group of one with state `()`, no group directory.
        s.cell("baseline", |m| {
            for i in 0..m {
                black_box(i);
            }
            Sample::with(m as u64, ToyRow { subject: "baseline", bytes: 0 })
        });
    })?;
    Ok(())
}
```

`cargo run --release --example toy -- --rounds 3 --out out/` writes five files:

```text
out/toy/small/vec_sum.csv
out/toy/small/fold_sum.csv
out/toy/large/vec_sum.csv
out/toy/large/fold_sum.csv
out/toy/baseline.csv
```

each with the same header and three rows, one per round:

```text
suite,group,name,round,position,group_position,cells,seed,batch,ops,elapsed_ns,ns_per_op,calibration,pilot_ns_per_op,start_ms,data.subject,data.bytes
toy,small,vec_sum,1,3,1,5,24301,1000000,1000000,212008000,212.008,Full,215.120,3105,vec_sum,8000
toy,small,vec_sum,2,0,0,5,24301,1000000,1000000,211730000,211.730,Full,215.120,6402,vec_sum,8000
toy,small,vec_sum,3,1,0,5,24301,1000000,1000000,212944000,212.944,Full,215.120,12811,vec_sum,8000
```

In round 1 the `small` group drew group slot 1 and `vec_sum` ran second within it, hence position 3 (the group before it held two cells); in round 2 the group drew slot 0 and `vec_sum` ran first. `--only toy/small/` rewrites the two `small` files and leaves the other three alone. The multi-suite shape: two directories with different rows, an engine built per visit and shared by the group's cells, a stopwatch for time inside the engine, and an op count that is what the engine did rather than what was asked.

```rust
use std::time::Duration;
use bensho::{Group, Harness, Sample, Stopwatch, Suite};

bensho::row! {
    /// One small-write batch: payload bytes pushed, packets the engine emitted
    /// (also the divisor of ns/op, via `Sample::ops`), time inside the engine's
    /// own calls, and the idle-connection count as a number so pandas can pivot.
    struct PacketRow { idle: u32, bytes: u64, packets: u64, engine_ns: u128 }
}

bensho::row! {
    /// One idle-tick batch: flows established and the process's peak RSS by
    /// the end of the batch (a run-wide high-water mark, not a delta).
    struct IdleRow { flows: u32, peak_rss_bytes: u64 }
}

struct Bed { engine: Engine, payload: Vec<u8>, clock: Stopwatch }

fn main() -> std::io::Result<()> {
    let harness = Harness::from_args();

    harness.suite("packets", |s: &mut Suite<PacketRow>| {
        for idle in [1u32, 100, 1000] {
            // Built each time the round reaches this group, dropped after its
            // last cell: three idle sizes, never more than one engine alive.
            let setup = move || Bed {
                engine: Engine::with_idle_connections(idle as usize),
                payload: vec![0xa5u8; 100],
                clock: Stopwatch::new(),
            };
            s.group(format!("lwip/idle{idle}"), setup, |g: &mut Group<PacketRow, Bed>| {
                g.cell("small-write", move |bed, m| {
                    let mut packets = 0u64;
                    for _ in 0..m {
                        // Only the engine call is on the stopwatch; the loop and
                        // the counter are harness overhead the CSV can subtract.
                        packets += bed.clock.time(|| bed.engine.write(&bed.payload)) as u64;
                    }
                    bed.engine.drain();
                    Sample::with(
                        packets,                                   // ns/op is per packet, not per write
                        PacketRow {
                            idle,
                            bytes: m as u64 * bed.payload.len() as u64,
                            packets,
                            engine_ns: bed.clock.take(),           // read and reset for the next batch
                        },
                    )
                });
                g.cell("bulk-write", move |bed, m| { /* the same shape over a 64 KiB payload */ });
            });
        }
    })?;

    harness.suite("idle", |s: &mut Suite<IdleRow>| {
        s.options_mut().budget = Duration::from_millis(500);    // ticks are cheap; this suite alone
        for flows in [100u32, 1000] {
            let setup = move || Peer::with_idle_connections(flows as usize);
            s.group(format!("peer/idle{flows}"), setup, |g: &mut Group<IdleRow, Peer>| {
                g.cell("tick", move |peer, m| {
                    for _ in 0..m {
                        peer.tick();
                    }
                    Sample::with(m as u64, IdleRow { flows, peak_rss_bytes: peak_rss_bytes() })
                });
            });
        }
    })?;
    Ok(())
}
```

Under `--out results/`:

```text
results/packets/lwip/idle1/small-write.csv
results/packets/lwip/idle1/bulk-write.csv
results/packets/lwip/idle100/small-write.csv
results/packets/lwip/idle100/bulk-write.csv
results/packets/lwip/idle1000/small-write.csv
results/packets/lwip/idle1000/bulk-write.csv
results/idle/peer/idle100/tick.csv
results/idle/peer/idle1000/tick.csv
```

One row from each suite:

```text
results/packets/lwip/idle100/small-write.csv
suite,group,name,round,position,group_position,cells,seed,batch,ops,elapsed_ns,ns_per_op,calibration,pilot_ns_per_op,start_ms,data.idle,data.bytes,data.packets,data.engine_ns
packets,lwip/idle100,small-write,1,3,1,6,24301,183422,183422,1998413000,10894.514,Budget,10731.002,4127,100,18342200,183422,1741220500

results/idle/peer/idle1000/tick.csv
suite,group,name,round,position,group_position,cells,seed,batch,ops,elapsed_ns,ns_per_op,calibration,pilot_ns_per_op,start_ms,data.flows,data.peak_rss_bytes
idle,peer/idle1000,tick,1,0,0,2,24301,1000000,1000000,412008000,412.008,Full,415.120,0,1000,88342528
```

In pandas over `results/packets/`: `data.engine_ns / ops` is time per packet inside the engine, `(elapsed_ns - data.engine_ns) / ops` is the loop and drain around it, `data.bytes / elapsed_ns` is payload throughput, and grouping on `data.idle` gives the sweep as lines. A closure that wants the harness stopwatch out of the picture entirely returns `Sample::with(..).timed(Duration::from_nanos(engine_ns as u64))` and `elapsed_ns` is then the engine time itself.

`bench --suite idle` never constructs an `Engine`. `bench --only packets/lwip/idle100/` builds one engine per round. `bench --skip lwip --out results/` runs the idle suite and, in the packets suite, nothing. `bench --list` prints the eight cells:

```text
packets/lwip/idle1/small-write
packets/lwip/idle1/bulk-write
packets/lwip/idle100/small-write
packets/lwip/idle100/bulk-write
packets/lwip/idle1000/small-write
packets/lwip/idle1000/bulk-write
idle/peer/idle100/tick
idle/peer/idle1000/tick
```

# Migration from the first spec

- `Bench::new(options)`, `Bench::run()` and `Bench::run_to(out)` go away; `Harness::suite(name, build)` replaces them, and `Bench` is renamed `Suite`. Tests write under a temporary `--out` and read the files back.
- `cell(subject, mode, op)` becomes `cell(name, op)` for a singleton or `group(subject, setup, |g| g.cell(mode, op))` when the subject's cells share a state. A bench that had two dimensions and no shared state joins them in the name (`chd/hit`) and, if it wants to pivot, repeats them as row fields.
- One file per cell instead of one per run. `subject` and `mode` columns become `suite`, `group` and `name`; `group_position` is new. User columns gain the `data.` prefix. The collision check is deleted.
- `--out FILE` becomes `--out DIR`; `--only PATTERN` keeps its substring semantics but matches `<suite>/<group>/<cell>`; `--suite` and `--skip` are new; `--no-header` is gone.
- `Options::keeps(subject, mode)` becomes `keeps(suite, path)`; `enters(suite)` is new.
- `README.md` and the first spec's API and CSV sections point here.

# Python side

- `Results.load` takes files or directories; a directory is read recursively (`**/*.csv`), and `source` is the file's path relative to the directory given (`lwip/idle100/small-write.csv`). `bensho-report results/packets/` reads a suite; `bensho-report results/` reads every suite, with the union of their `data.` columns and NaN where a suite lacks one. `group` is read as a string column with the empty string for singletons.
- `BENSHO_COLUMNS` is the new list; `user_columns` is every column starting with `data.`, which no longer needs a subtraction.
- `census()` is per file: rows present against the rounds the suite reached, the group, and the distinct `seed`/`cells` values seen, so a directory assembled from several runs is reported as such rather than averaged silently.
- `cells()` and `anomalies()` key on `(suite, group, name)`. `round_drift()` is unchanged in meaning.
- `position_drift(level)` and `carryover(level)` take the level the slot or predecessor is read at: `"round"` (the default; the cell's slot in the flattened round, the predecessor is whatever ran before it, as before), `"group"` (the group's slot among the round's groups; the predecessor is the previous group), `"cell"` (the cell's slot among its group's cells, from ranking `position` within `(round, group)`; the predecessor is the previous cell of the same group, `(first)` for the one that ran on the fresh state). Groups of one contribute no slot at level `"cell"`.
- `figure(color_by=None)`: cells along x by path, the colour column a parameter, typically `group` or a `data.` column such as `data.subject`; with `None` every cell is one colour. `bensho-report --color-by group`.
- Dotted names need brackets in pandas: `df["data.bytes"]` works, `df.data.bytes` does not, and `query` wants backticks, `` df.query("`data.idle` == 100") ``. The notebook and `report.py` use bracket access throughout.

# Tests to add

- A two-suite program with different row types under a temporary `out_dir`: two directories, one file per cell, every file in a suite with the same header, `data.`-prefixed user columns in field order, `Report.suite` set, rows per file equal to `rounds`.
- Name validation for suites, groups and cells: `../x`, `/abs/x`, `a//b`, `x.csv` and an empty name are errors before anything runs; `a/b` creates the subdirectory; the same suite name twice, or the same cell path twice in one suite, is an error.
- Partial rerun: run all, then run with `--only` for one cell; the other files are byte-for-byte untouched and the selected file is rewritten. The same with `--only <suite>/<group>/` selecting a group.
- `--suite other` skips a suite whose build closure panics, proving the closure did not run.
- Filter table: `--only`, `--skip`, and their combination against a fixed cell list, matched on `<suite>/<group>/<cell>`.
- `--list` prints `<suite>/<group>/<cell>` per cell, runs no pilot and no setup (closures that panic if called) and creates nothing.
- A `Row` field named `ops` lands as `data.ops` next to bensho's `ops`, both present, both correct.
- Nested shuffle determinism: the same seed and round give the same group order and the same inner orders; adding a group leaves earlier groups' inner orders unchanged; changing the round count leaves earlier rounds unchanged, read back from the recorded `position`/`group_position`.
- Exactly one state alive at a time: `setup` increments a counter the state's `Drop` decrements, and every cell asserts the counter is one.
- `setup` runs once per group per round plus once for calibration; `start_ms` and `position` are consistent with the recorded orders.
- The existing schedule and calibration tests are unchanged.

# Non-goals

- Cross-suite interleaving; make them one suite.
- Latin-square or Williams scheduling at either level, for the reason in the first spec.
- Globs or regular expressions in filters. Substrings on `<suite>/<group>/<cell>` cover what `cargo test` covers, with no dependency.
- One file per suite, appending, or stdout output; the loader concatenates, and the per-cell file is what makes partial reruns compose.
- Naming the columns at the emit site instead of in a `row!` struct. Considered (it is how `tracing` and `slog` work); rejected because only a shared type lets the compiler prove every cell of a suite emits the same columns, and the struct costs one declaration.
- Per-cell setup hooks or a `Batch` handle; the group's `setup` and `Sample::timed` are the two seams.
- Parallel or process-isolated cells.
- Config files or environment variables; the CLI and the code are the two places a run is described.
