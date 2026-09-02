//! The spec's claims, checked: the two schedules, the state lifetime, the
//! calibration rule, the files and their headers, the filters, `--list`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bensho::{
    calibrate, schedule, schedule_in_group, Calibration, Group, Harness, Options, Sample, Suite,
    COLUMNS,
};

/// A fresh directory under the system temp dir, named after the test.
fn out_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bensho-test-{}-{test}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Small, fast options plus `--out DIR` and whatever else the test wants.
fn harness(dir: &Path, args: &[&str]) -> Harness {
    let mut all = vec![
        "--rounds",
        "2",
        "--ops",
        "10",
        "--min-ops",
        "1",
        "--pilot",
        "2",
        "--seed",
        "77",
        "--out",
    ];
    let dir = dir.to_str().unwrap();
    all.push(dir);
    all.extend(args);
    Harness::new(Options::parse(all).unwrap())
}

fn read(dir: &Path, rel: &str) -> String {
    fs::read_to_string(dir.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn files(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_str().unwrap().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

// ---- schedules ------------------------------------------------------------

#[test]
fn schedule_is_a_pure_function_of_seed_and_round() {
    assert_eq!(schedule(7, 1, 12), schedule(7, 1, 12));
    assert_eq!(schedule(0x5eed, 3, 40), schedule(0x5eed, 3, 40));
    assert_eq!(schedule_in_group(7, 1, 2, 9), schedule_in_group(7, 1, 2, 9));
}

#[test]
fn schedules_are_permutations() {
    for round in 1..=6 {
        let mut order = schedule(99, round, 17);
        order.sort_unstable();
        assert_eq!(order, (0..17).collect::<Vec<_>>());
        let mut inner = schedule_in_group(99, round, 3, 11);
        inner.sort_unstable();
        assert_eq!(inner, (0..11).collect::<Vec<_>>());
    }
    assert_eq!(schedule(1, 1, 0), Vec::<usize>::new());
    assert_eq!(schedule(1, 1, 1), vec![0]);
    assert_eq!(schedule_in_group(1, 1, 0, 1), vec![0]);
}

#[test]
fn rounds_differ_seeds_differ_groups_differ() {
    let rounds: Vec<Vec<usize>> = (1..=6).map(|r| schedule(42, r, 10)).collect();
    for (i, a) in rounds.iter().enumerate() {
        for b in &rounds[i + 1..] {
            assert_ne!(a, b, "two rounds drew the same order");
        }
    }
    assert_ne!(schedule(1, 1, 10), schedule(2, 1, 10));
    // the group-level stream and the first group's inner stream differ
    assert_ne!(schedule(42, 1, 10), schedule_in_group(42, 1, 0, 10));
    assert_ne!(
        schedule_in_group(42, 1, 0, 10),
        schedule_in_group(42, 1, 1, 10)
    );
}

#[test]
fn no_slot_is_owned_by_one_group() {
    let groups = 6;
    let mut firsts = vec![0u32; groups];
    let mut lasts = vec![0u32; groups];
    for round in 1..=200 {
        let order = schedule(0xabc, round, groups);
        firsts[order[0]] += 1;
        lasts[order[groups - 1]] += 1;
    }
    assert!(firsts.iter().all(|&n| n > 0), "{firsts:?}");
    assert!(lasts.iter().all(|&n| n > 0), "{lasts:?}");
}

/// Run `groups` groups of three cells each for `rounds` rounds and return
/// `(round, group_position, position, group, name)` per record.
fn nested_run(test: &str, groups: usize, rounds: &str) -> Vec<(u32, usize, usize, String, String)> {
    let dir = out_dir(test);
    let h = harness(&dir, &["--rounds", rounds]);
    let report = h
        .suite("s", |s: &mut Suite<()>| {
            for g in 0..groups {
                s.group(
                    format!("g{g}"),
                    || (),
                    |g: &mut Group<(), ()>| {
                        for c in ["a", "b", "c"] {
                            g.cell(c, |_, m| Sample::ops(m as u64));
                        }
                    },
                );
            }
        })
        .unwrap()
        .unwrap();
    report
        .records
        .iter()
        .map(|r| {
            (
                r.round,
                r.group_position,
                r.position,
                r.group.clone(),
                r.name.clone(),
            )
        })
        .collect()
}

#[test]
fn nested_shuffle_is_deterministic_and_stable_under_growth() {
    let a = nested_run("nested-a", 4, "3");
    let b = nested_run("nested-b", 4, "3");
    assert_eq!(a, b, "same seed and round give the same orders");
    assert_eq!(a.len(), 36);

    // positions are the flattened execution order; group_position is
    // constant within a group's visit and steps by one per group
    for (i, (round, gpos, pos, _, _)) in a.iter().enumerate() {
        assert_eq!(*round as usize, i / 12 + 1);
        assert_eq!(*pos, i % 12);
        assert_eq!(*gpos, (i % 12) / 3);
    }
    let group_orders: Vec<Vec<String>> = (1..=3)
        .map(|r| {
            let mut gs: Vec<String> = a.iter().filter(|x| x.0 == r).map(|x| x.3.clone()).collect();
            gs.dedup();
            gs
        })
        .collect();
    assert_eq!(
        group_orders[0].len(),
        4,
        "each group visited once per round"
    );
    assert_ne!(group_orders[0], group_orders[1]);

    // adding a fifth group leaves the four earlier groups' inner orders alone
    let five = nested_run("nested-c", 5, "3");
    let inner = |run: &[(u32, usize, usize, String, String)], round: u32, group: &str| {
        run.iter()
            .filter(|x| x.0 == round && x.3 == group)
            .map(|x| x.4.clone())
            .collect::<Vec<_>>()
    };
    for round in 1..=3 {
        for g in 0..4 {
            let name = format!("g{g}");
            assert_eq!(inner(&a, round, &name), inner(&five, round, &name));
        }
    }
    // the inner orders differ from one another somewhere: the second stream is live
    let inners: Vec<Vec<String>> = (0..4)
        .flat_map(|g| (1..=3).map(move |r| (g, r)))
        .map(|(g, r)| inner(&a, r, &format!("g{g}")))
        .collect();
    assert!(inners.iter().any(|o| o != &inners[0]), "{inners:?}");

    // more rounds leave earlier rounds byte-for-byte unchanged
    let long = nested_run("nested-d", 4, "5");
    assert_eq!(long.len(), 60);
    assert_eq!(&long[..36], &a[..]);
}

// ---- state per visit -------------------------------------------------------

static ALIVE: AtomicUsize = AtomicUsize::new(0);
static BUILT: AtomicUsize = AtomicUsize::new(0);

struct Counted;

impl Drop for Counted {
    fn drop(&mut self) {
        ALIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn exactly_one_state_alive_and_setup_once_per_visit() {
    let dir = out_dir("state");
    let h = harness(&dir, &["--rounds", "3"]);
    let setup = || {
        BUILT.fetch_add(1, Ordering::SeqCst);
        ALIVE.fetch_add(1, Ordering::SeqCst);
        Counted
    };
    let report = h
        .suite("s", |s: &mut Suite<()>| {
            for g in ["g1", "g2", "g3"] {
                s.group(g, setup, |g: &mut Group<(), Counted>| {
                    for c in ["a", "b"] {
                        g.cell(c, |_state, m| {
                            assert_eq!(ALIVE.load(Ordering::SeqCst), 1, "one state alive");
                            Sample::ops(m as u64)
                        });
                    }
                });
            }
            // a singleton runs between groups with no state of theirs alive
            s.cell("solo", |m| {
                assert_eq!(ALIVE.load(Ordering::SeqCst), 0);
                Sample::ops(m as u64)
            });
        })
        .unwrap()
        .unwrap();
    assert_eq!(ALIVE.load(Ordering::SeqCst), 0, "everything dropped");
    // three groups x (one calibration visit + three rounds)
    assert_eq!(BUILT.load(Ordering::SeqCst), 3 * 4);
    assert_eq!(report.records.len(), 3 * 7);
    assert!(report.records.iter().all(|r| r.cells == 7));
}

#[test]
fn a_group_shares_its_state_within_a_round_and_starts_fresh_each_round() {
    let dir = out_dir("share");
    let h = harness(&dir, &["--rounds", "3"]);
    bensho::row! { struct Seen { before: u32 } }
    let report = h
        .suite("s", |s: &mut Suite<Seen>| {
            s.group(
                "g",
                || 0u32,
                |g: &mut Group<Seen, u32>| {
                    for c in ["a", "b", "c"] {
                        g.cell(c, |count, m| {
                            let before = *count;
                            *count += 1;
                            Sample::with(m as u64, Seen { before })
                        });
                    }
                },
            );
        })
        .unwrap()
        .unwrap();
    // within a round the three cells see 0, 1, 2 in their inner order; the
    // next round starts from a fresh 0 again
    assert_eq!(report.records.len(), 9);
    let text = read(&dir, "s/g/a.csv") + &read(&dir, "s/g/b.csv") + &read(&dir, "s/g/c.csv");
    for round in 1..=3 {
        let mut befores: Vec<u32> = text
            .lines()
            .filter(|l| l.starts_with("s,g,") && l.split(',').nth(3) == Some(&round.to_string()))
            .map(|l| l.rsplit(',').next().unwrap().parse().unwrap())
            .collect();
        befores.sort_unstable();
        assert_eq!(befores, vec![0, 1, 2], "round {round}");
    }
}

// ---- files ---------------------------------------------------------------

#[test]
fn two_suites_two_row_types_one_file_per_cell() {
    bensho::row! { struct A { tag: &'static str, note: Option<u32> } }
    bensho::row! { struct B { flows: u32 } }
    let dir = out_dir("two-suites");
    let h = harness(&dir, &["--rounds", "3"]);

    let a = h
        .suite("netstack/packets", |s: &mut Suite<A>| {
            s.group(
                "lwip",
                || (),
                |g: &mut Group<A, ()>| {
                    g.cell("hit", |_, m| {
                        Sample::with(
                            m as u64,
                            A {
                                tag: "h",
                                note: Some(1),
                            },
                        )
                    });
                    g.cell("miss", |_, m| {
                        Sample::with(
                            m as u64,
                            A {
                                tag: "m",
                                note: None,
                            },
                        )
                    });
                },
            );
            s.cell("solo", |m| {
                Sample::with(
                    m as u64,
                    A {
                        tag: "s",
                        note: Some(2),
                    },
                )
            });
        })
        .unwrap()
        .unwrap();
    assert_eq!(a.suite, "netstack/packets");
    assert_eq!(a.calibrations.len(), 3);
    assert_eq!(a.records.len(), 9);

    let b = h
        .suite("idle", |s: &mut Suite<B>| {
            s.cell("peer/idle100", |m| Sample::with(m as u64, B { flows: 100 }));
        })
        .unwrap()
        .unwrap();
    assert_eq!(b.suite, "idle");

    assert_eq!(
        files(&dir),
        vec![
            "idle/peer/idle100.csv",
            "netstack/packets/lwip/hit.csv",
            "netstack/packets/lwip/miss.csv",
            "netstack/packets/solo.csv",
        ]
    );
    let header_a: Vec<String> = COLUMNS
        .iter()
        .map(|c| c.to_string())
        .chain(["data.tag".into(), "data.note".into()])
        .collect();
    for rel in [
        "netstack/packets/lwip/hit.csv",
        "netstack/packets/lwip/miss.csv",
        "netstack/packets/solo.csv",
    ] {
        let text = read(&dir, rel);
        let mut lines = text.lines();
        assert_eq!(lines.next().unwrap(), header_a.join(","), "{rel}");
        let rows: Vec<&str> = lines.collect();
        assert_eq!(rows.len(), 3, "{rel}: one row per round");
        for row in &rows {
            assert_eq!(row.split(',').count(), header_a.len(), "{row}");
        }
    }
    let hit = read(&dir, "netstack/packets/lwip/hit.csv");
    assert!(hit
        .lines()
        .nth(1)
        .unwrap()
        .starts_with("netstack/packets,lwip,hit,1,"));
    assert!(hit.lines().nth(1).unwrap().ends_with(",h,1"));
    let miss = read(&dir, "netstack/packets/lwip/miss.csv");
    assert!(
        miss.lines().nth(1).unwrap().ends_with(",m,"),
        "None is the empty cell"
    );
    let solo = read(&dir, "netstack/packets/solo.csv");
    assert!(
        solo.lines()
            .nth(1)
            .unwrap()
            .starts_with("netstack/packets,,solo,1,"),
        "empty group"
    );

    let idle = read(&dir, "idle/peer/idle100.csv");
    let header_b: Vec<String> = COLUMNS
        .iter()
        .map(|c| c.to_string())
        .chain(["data.flows".into()])
        .collect();
    assert_eq!(idle.lines().next().unwrap(), header_b.join(","));
    assert_eq!(idle.lines().count(), 4);
}

#[test]
fn a_row_field_named_ops_lands_under_data() {
    bensho::row! { struct Clash { ops: u64, seed: &'static str } }
    let dir = out_dir("clash");
    let h = harness(&dir, &["--rounds", "1"]);
    h.suite("s", |s: &mut Suite<Clash>| {
        s.cell("c", |m| {
            Sample::with(
                m as u64 * 2,
                Clash {
                    ops: 999,
                    seed: "x",
                },
            )
        });
    })
    .unwrap()
    .unwrap();
    let text = read(&dir, "s/c.csv");
    let header: Vec<&str> = text.lines().next().unwrap().split(',').collect();
    assert_eq!(&header[header.len() - 2..], &["data.ops", "data.seed"]);
    let row: Vec<&str> = text.lines().nth(1).unwrap().split(',').collect();
    let col = |name: &str| row[header.iter().position(|h| *h == name).unwrap()];
    assert_eq!(col("ops"), "20");
    assert_eq!(col("data.ops"), "999");
    assert_eq!(col("seed"), "77");
    assert_eq!(col("data.seed"), "x");
}

#[test]
fn partial_reruns_leave_other_files_untouched() {
    let dir = out_dir("rerun");
    let program = |h: &Harness| {
        h.suite("s", |s: &mut Suite<()>| {
            s.group(
                "g",
                || (),
                |g: &mut Group<(), ()>| {
                    g.cell("a", |_, m| Sample::ops(m as u64));
                    g.cell("b", |_, m| Sample::ops(m as u64));
                },
            );
            s.cell("c", |m| Sample::ops(m as u64));
        })
        .unwrap()
    };
    program(&harness(&dir, &[]));
    let before: Vec<String> = ["s/g/a.csv", "s/g/b.csv", "s/c.csv"]
        .iter()
        .map(|f| read(&dir, f))
        .collect();

    // one cell: only its file changes (cells and seed differ, so the bytes do)
    program(&harness(&dir, &["--only", "s/g/a", "--seed", "78"]));
    assert_ne!(read(&dir, "s/g/a.csv"), before[0]);
    assert_eq!(read(&dir, "s/g/b.csv"), before[1]);
    assert_eq!(read(&dir, "s/c.csv"), before[2]);
    assert!(
        read(&dir, "s/g/a.csv").contains(",1,78,"),
        "cells=1, seed=78"
    );

    // a group by prefix: both of its files change, the singleton does not
    let a1 = read(&dir, "s/g/a.csv");
    program(&harness(&dir, &["--only", "s/g/", "--seed", "79"]));
    assert_ne!(read(&dir, "s/g/a.csv"), a1);
    assert_ne!(read(&dir, "s/g/b.csv"), before[1]);
    assert_eq!(read(&dir, "s/c.csv"), before[2]);
    assert_eq!(files(&dir).len(), 3);
}

// ---- names ---------------------------------------------------------------

#[test]
fn names_are_validated_before_anything_runs() {
    for bad in ["", "../x", "/abs/x", "a//b", "x.csv", "a/./b", "a/../b"] {
        assert!(bensho::check_name("cell", bad).is_err(), "{bad:?}");
    }
    for good in ["a", "a/b", "chd/hit", "small-write/idle100", "x.csv.gz"] {
        assert!(bensho::check_name("cell", good).is_ok(), "{good:?}");
    }

    let dir = out_dir("names");
    let h = harness(&dir, &[]);
    assert!(h.suite("../x", |_: &mut Suite<()>| {}).is_err());
    assert!(h.suite("x.csv", |_: &mut Suite<()>| {}).is_err());

    // a bad cell name is an error out of `suite`, and no pilot ran
    let err = h
        .suite("s1", |s: &mut Suite<()>| {
            s.cell("a//b", |_| panic!("never piloted"));
        })
        .unwrap_err();
    assert!(err.to_string().contains("a//b"), "{err}");
    let err = h
        .suite("s2", |s: &mut Suite<()>| {
            s.group(
                "bad.csv",
                || (),
                |g: &mut Group<(), ()>| {
                    g.cell("x", |_, _| panic!("never piloted"));
                },
            );
        })
        .unwrap_err();
    assert!(err.to_string().contains("bad.csv"), "{err}");
    assert!(files(&dir).is_empty(), "nothing created");

    // duplicates: suite twice, cell path twice (group a / cell b vs cell a/b)
    h.suite("dup", |s: &mut Suite<()>| {
        s.cell("x", |m| Sample::ops(m as u64));
    })
    .unwrap();
    let err = h.suite("dup", |_: &mut Suite<()>| {}).unwrap_err();
    assert!(err.to_string().contains("twice"), "{err}");
    let err = h
        .suite("dupcell", |s: &mut Suite<()>| {
            s.group(
                "a",
                || (),
                |g: &mut Group<(), ()>| {
                    g.cell("b", |_, m| Sample::ops(m as u64));
                },
            );
            s.cell("a/b", |m| Sample::ops(m as u64));
        })
        .unwrap_err();
    assert!(err.to_string().contains("a/b"), "{err}");
    // two groups may share a cell name
    h.suite("shared", |s: &mut Suite<()>| {
        for g in ["g1", "g2"] {
            s.group(
                g,
                || (),
                |g: &mut Group<(), ()>| {
                    g.cell("hit", |_, m| Sample::ops(m as u64));
                },
            );
        }
    })
    .unwrap()
    .unwrap();
}

// ---- filters -------------------------------------------------------------

#[test]
fn suite_filter_is_a_pre_skip() {
    let dir = out_dir("suite-filter");
    let h = harness(&dir, &["--suite", "other"]);
    let r = h
        .suite("s", |_: &mut Suite<()>| panic!("build must not run"))
        .unwrap();
    assert!(r.is_none());
    assert!(files(&dir).is_empty());
    let r = h
        .suite("another", |s: &mut Suite<()>| {
            s.cell("x", |m| Sample::ops(m as u64));
        })
        .unwrap();
    assert!(r.is_some());
}

#[test]
fn only_and_skip_match_the_full_path() {
    let cells = [
        "packets/chd/hit",
        "packets/chd/miss",
        "packets/lwip/hit",
        "idle/peer/idle100",
    ];
    let table: &[(&[&str], &[&str])] = &[
        (&[], &cells),
        (
            &["--only", "packets/chd"],
            &["packets/chd/hit", "packets/chd/miss"],
        ),
        (
            &["--only", "packets/chd/"],
            &["packets/chd/hit", "packets/chd/miss"],
        ),
        (
            &["--only", "/hit"],
            &["packets/chd/hit", "packets/lwip/hit"],
        ),
        (
            &["--only", "chd", "--only", "idle"],
            &["packets/chd/hit", "packets/chd/miss", "idle/peer/idle100"],
        ),
        (
            &["--skip", "chd"],
            &["packets/lwip/hit", "idle/peer/idle100"],
        ),
        (
            &["--only", "packets", "--skip", "miss"],
            &["packets/chd/hit", "packets/lwip/hit"],
        ),
        (&["--only", "nothing"], &[]),
    ];
    for (args, expected) in table {
        let o = Options::parse(args.iter().map(|s| s.to_string())).unwrap();
        let kept: Vec<&str> = cells
            .iter()
            .copied()
            .filter(|c| {
                let (suite, path) = c.split_once('/').unwrap();
                o.keeps(suite, path)
            })
            .collect();
        assert_eq!(&kept, expected, "{args:?}");
    }
    assert!(Options::parse(["--suite", "pack"])
        .unwrap()
        .enters("packets"));
    assert!(!Options::parse(["--suite", "pack"]).unwrap().enters("idle"));
}

#[test]
fn a_group_whose_cells_are_all_skipped_is_never_set_up() {
    let dir = out_dir("skip-group");
    let h = harness(&dir, &["--skip", "s/heavy/"]);
    let report = h
        .suite("s", |s: &mut Suite<()>| {
            s.group(
                "heavy",
                || panic!("setup must not run"),
                |g: &mut Group<(), ()>| {
                    g.cell("x", |_, m| Sample::ops(m as u64));
                },
            );
            s.cell("light", |m| Sample::ops(m as u64));
        })
        .unwrap()
        .unwrap();
    assert_eq!(report.records.len(), 2);
    assert_eq!(files(&dir), vec!["s/light.csv"]);
}

#[test]
fn list_prints_paths_and_runs_nothing() {
    let dir = out_dir("list");
    let h = harness(&dir, &["--list"]);
    let r = h
        .suite("s", |s: &mut Suite<()>| {
            s.group(
                "g",
                || panic!("no setup under --list"),
                |g: &mut Group<(), ()>| {
                    g.cell("a", |_, _| panic!("no pilot under --list"));
                },
            );
            s.cell("b", |_| panic!("no pilot under --list"));
        })
        .unwrap();
    assert!(r.is_none());
    assert!(files(&dir).is_empty());
}

// ---- calibration and measurement -------------------------------------------

fn options(args: &[&str]) -> Options {
    Options::parse(args.iter().map(|s| s.to_string())).unwrap()
}

#[test]
fn calibration_rule() {
    let o = options(&[
        "--ops",
        "1000000",
        "--min-ops",
        "1000",
        "--budget-ms",
        "2000",
    ]);
    assert_eq!(calibrate(1.0, &o), (1_000_000, Calibration::Full));
    assert_eq!(calibrate(2000.0, &o), (1_000_000, Calibration::Full));
    assert_eq!(calibrate(10_000.0, &o), (200_000, Calibration::Budget));
    assert_eq!(calibrate(1_000_000.0, &o), (2_000, Calibration::Budget));
    assert_eq!(calibrate(1e9, &o), (1_000, Calibration::Floor));
    assert_eq!(calibrate(0.0, &o), (1_000_000, Calibration::Full));
    let small = options(&["--ops", "10", "--min-ops", "1000"]);
    assert_eq!(calibrate(1e9, &small), (10, Calibration::Floor));
}

#[test]
fn calibration_is_fixed_before_round_one_and_recorded() {
    let dir = out_dir("calibration");
    let h = harness(
        &dir,
        &[
            "--rounds",
            "3",
            "--ops",
            "100000",
            "--min-ops",
            "10",
            "--pilot",
            "8",
            "--budget-ms",
            "20",
        ],
    );
    let report = h
        .suite("s", |s: &mut Suite<()>| {
            // slow: ~1 ms per requested op, so 20 ms buys ~20 ops
            s.cell("slow", |m| {
                std::thread::sleep(Duration::from_millis(m as u64));
                Sample::ops(m as u64)
            });
            // fast: full ceiling, and ops is the closure's count
            s.cell("fast", |m| Sample::ops(m as u64 * 3));
        })
        .unwrap()
        .unwrap();
    let slow: Vec<_> = report.records.iter().filter(|r| r.name == "slow").collect();
    assert_eq!(slow.len(), 3);
    assert!(
        slow.iter().all(|r| r.calibration == Calibration::Budget),
        "{slow:?}"
    );
    assert!(slow.iter().all(|r| r.batch == slow[0].batch));
    assert!(
        slow[0].batch >= 10 && slow[0].batch < 100_000,
        "{}",
        slow[0].batch
    );
    let fast: Vec<_> = report.records.iter().filter(|r| r.name == "fast").collect();
    assert!(fast
        .iter()
        .all(|r| r.calibration == Calibration::Full && r.batch == 100_000));
    assert!(fast.iter().all(|r| r.ops == 300_000));
    let text = read(&dir, "s/slow.csv");
    assert!(text.lines().nth(1).unwrap().contains(",Budget,"));
    let summary = report.summary();
    assert!(summary.starts_with("== s\n"));
    assert!(summary.contains("slow") && summary.contains("Budget"));
}

#[test]
fn zero_ops_is_a_bench_bug() {
    let dir = out_dir("zero");
    let h = harness(&dir, &[]);
    let err = h
        .suite("s", |s: &mut Suite<()>| {
            s.group(
                "g",
                || (),
                |g: &mut Group<(), ()>| {
                    g.cell("z", |_, _| Sample::ops(0));
                },
            );
        })
        .unwrap_err();
    assert!(err.to_string().contains("s/g/z"), "{err}");
}

#[test]
fn own_elapsed_overrides_the_harness_stopwatch() {
    let dir = out_dir("elapsed");
    let h = harness(&dir, &["--rounds", "1", "--ops", "4", "--pilot", "1"]);
    let report = h
        .suite("s", |s: &mut Suite<()>| {
            s.cell("c", |m| {
                std::thread::sleep(Duration::from_millis(5));
                Sample::ops(m as u64).timed(Duration::from_nanos(400))
            });
        })
        .unwrap()
        .unwrap();
    assert_eq!(report.records[0].elapsed_ns, 400);
    assert_eq!(report.records[0].ns_per_op, 100.0);
}

#[test]
fn suite_options_are_the_suite_s_alone() {
    let dir = out_dir("suite-options");
    let h = harness(&dir, &["--rounds", "2"]);
    let r = h
        .suite("s", |s: &mut Suite<()>| {
            s.options_mut().rounds = 4;
            s.cell("c", |m| Sample::ops(m as u64));
        })
        .unwrap()
        .unwrap();
    assert_eq!(r.records.len(), 4);
    assert_eq!(h.options().rounds, 2);
}

#[test]
fn csv_quotes_only_what_needs_it() {
    assert_eq!(bensho::quote("plain"), "plain");
    assert_eq!(bensho::quote("a,b"), "\"a,b\"");
    assert_eq!(bensho::quote("say \"hi\""), "\"say \"\"hi\"\"\"");
    assert_eq!(bensho::quote(""), "");
}

#[test]
fn options_leave_the_rest_for_the_bench() {
    let o = options(&[
        "--seed",
        "0xff",
        "--rounds",
        "2",
        "--large-mib",
        "8",
        "positional",
        "--list",
        "--out",
        "o",
    ]);
    assert_eq!(o.seed, 255);
    assert_eq!(o.rounds, 2);
    assert!(o.list);
    assert_eq!(o.out_dir, PathBuf::from("o"));
    assert_eq!(o.rest, vec!["--large-mib", "8", "positional"]);
    assert!(Options::parse(["--rounds", "0"]).is_err());
    assert!(Options::parse(["--rounds"]).is_err());
    assert!(Options::parse(["--ops", "ten"]).is_err());
    assert!(Options::parse(["--no-header"]).unwrap().rest == vec!["--no-header"]);
}

#[test]
fn stopwatch_accumulates() {
    let mut sw = bensho::Stopwatch::new();
    let v = sw.time(|| {
        std::thread::sleep(Duration::from_millis(2));
        7
    });
    assert_eq!(v, 7);
    assert!(sw.ns() >= 2_000_000);
    assert!(sw.take() >= 2_000_000);
    assert_eq!(sw.ns(), 0);
}
