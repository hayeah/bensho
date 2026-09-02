---
title: Worklog - bensho suites, files per cell, and groups with state built per visit
date: 2026-09-02
tags: [bensho, benchmark, csv, suite, group, harness, rust, python, worklog]
status: both specs implemented, gates green, toy bench and notebook run end to end
related:
  - specs/20260902_182536_bensho_suite_csv_sink_and_filter.md
  - specs/20260902_180344_bensho_micro_benchmark_harness.md
  - worklogs/20260902_181220_bensho.md
---

# What was built

- The spec amended with the group level before any code: a group sits between suite and cell, its state built by `setup` each time a round reaches it and dropped after its last cell, so exactly one state is alive at a time and every round measures a fresh one. Committed first, then implemented.
- `Harness` (the program: options, `--out DIR`, `suite()`), `Suite` (one row type; `cell` for singletons, `group(name, setup, build)` for shared state, `options_mut` per suite), `Group::cell(name, op)` with `op(&mut S, batch)`. Groups are boxed behind a `Visit` trait inside the suite, so one suite mixes state types; the row type is the suite's.
- The nested shuffle: `schedule(seed, round, groups)` orders groups with `SplitMix(seed ^ round)`, `schedule_in_group(seed, round, group_index, cells)` orders one group's cells with `SplitMix(seed ^ round ^ ((group_index + 1) << 32))`, `group_index` the registration index. Adding a group leaves earlier groups' inner orders alone; adding rounds leaves earlier rounds alone; the `+ 1` keeps the first group's stream distinct from the group-level one.
- Files: `<out>/<suite>/<group>/<cell>.csv` (`<out>/<suite>/<cell>.csv` for singletons), header on create, one open-append-close per row, so partial reruns compose on disk and `--only <suite>/<group>/` selects a group with no new grammar.
- Columns: `suite,group,name,round,position,group_position,cells,seed,batch,ops,elapsed_ns,ns_per_op,calibration,pilot_ns_per_op,start_ms`, then `data.<field>`. `suite` was added beyond the amendment brief: without it a directory holding several suites cannot reconstruct each suite's rounds, since the file path is ambiguous once suite and group names may contain slashes. `group` is the empty string for singletons; the loader reads it back as the empty string.
- Removed: `Bench`, `run_to`, the public CSV writer, `--no-header`, stdout output, the user-column collision rule.
- Python: `Results.load` takes files or directories, `census()` is per file with the seeds and round sizes seen, keys are `(suite, group, name)`, `position_drift(level)` and `carryover(level)` at `round`, `group` and `cell`, `figure(color_by)`, `tables()` feeding both the report and `dump`. `bensho-report PATH... --color-by group`. The notebook reads a results directory.

# Measured

The toy bench in release, 3 rounds, 200 ms budget, on the development machine: `small/vec_sum` and `small/fold_sum` at 55.0 ns/op (`Full`, 1,000,000 batch), `large/vec_sum` and `large/fold_sum` at 72.4 and 72.7 us/op (`Budget`, batches 2,262 and 2,639), `baseline` at 0.25 ns/op. Round drift within 0.5%. The cell-level position drift is the one to watch for a real bench: it reads the slot a cell drew within its group's visit, which is what earlier cells left in the freshly built state.

# Traps

- The group-level carry-over merge cross-multiplies unless the predecessor frame is reduced to one row per `(round, group_position)`: every cell of the previous group names the same predecessor, and merging them all gave `cells x cells` rows per visit. Caught by the synthetic-order test.
- A closure-parameter annotation like `|s: &mut Suite<ToyRow>|` needs the suite's closure lifetime to be a generic of `Harness::suite` (`suite<'a, R: Row + 'a>`), not higher-ranked; a `for<'a>` bound would force every registered op to be `'static`.
- `census()` expects `rounds x cells-in-file`; a frame that is not one file per cell (a test frame, or a concatenated CSV) still reports correctly.

# Line counts

- Rust: `src/` 1,079 (suite 370, record 181, options 146, row 82, harness 76, sample 73, csv 67, rng 52, lib 32), `tests/harness.rs` 847, `examples/toy.rs` 72.
- Python: `results.py` 415, `results_test.py` 245, `results_notebook.py` 104, `report.py` 55.

# Open

- `--list` output format (`<suite>/<group>/<cell>` per line) is checked by running the example, not by an integration test: examples have no `CARGO_BIN_EXE_` and the harness prints straight to stdout.
- The `Floor` verdict is still tested through `calibrate` only.
- `figure()` caps at eight colour values; faceting is the caller's.
- The reference CSV rows in the spec's worked examples are illustrative, not pasted from a run.
