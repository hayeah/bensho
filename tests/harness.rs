//! The spec's claims, checked: shuffle determinism, the calibration rule,
//! the CSV shape.

use std::time::Duration;

use bensho::{calibrate, schedule, Bench, Calibration, Options, Sample, COLUMNS};

fn options(args: &[&str]) -> Options {
    Options::parse(args.iter().map(|s| s.to_string())).unwrap()
}

#[test]
fn schedule_is_a_pure_function_of_seed_and_round() {
    assert_eq!(schedule(7, 1, 12), schedule(7, 1, 12));
    assert_eq!(schedule(0x5eed, 3, 40), schedule(0x5eed, 3, 40));
}

#[test]
fn schedule_is_a_permutation() {
    for round in 1..=6 {
        let mut order = schedule(99, round, 17);
        order.sort_unstable();
        assert_eq!(order, (0..17).collect::<Vec<_>>());
    }
    assert_eq!(schedule(1, 1, 0), Vec::<usize>::new());
    assert_eq!(schedule(1, 1, 1), vec![0]);
}

#[test]
fn rounds_differ_and_seeds_differ() {
    let rounds: Vec<Vec<usize>> = (1..=6).map(|r| schedule(42, r, 10)).collect();
    for (i, a) in rounds.iter().enumerate() {
        for b in &rounds[i + 1..] {
            assert_ne!(a, b, "two rounds drew the same order");
        }
    }
    assert_ne!(schedule(1, 1, 10), schedule(2, 1, 10));
}

#[test]
fn no_slot_is_owned_by_one_cell() {
    // Over many rounds every cell should visit the first and last slot.
    let cells = 6;
    let mut firsts = vec![0u32; cells];
    let mut lasts = vec![0u32; cells];
    for round in 1..=200 {
        let order = schedule(0xabc, round, cells);
        firsts[order[0]] += 1;
        lasts[order[cells - 1]] += 1;
    }
    assert!(firsts.iter().all(|&n| n > 0), "{firsts:?}");
    assert!(lasts.iter().all(|&n| n > 0), "{lasts:?}");
}

#[test]
fn changing_the_round_count_leaves_earlier_rounds_unchanged() {
    // Two runs of the same cells, three rounds and five rounds: rounds 1..3
    // of the longer run are byte-for-byte the shorter run's rounds.
    let run = |rounds: &str| {
        let mut out = Vec::new();
        let mut bench = Bench::<()>::new(options(&[
            "--rounds",
            rounds,
            "--ops",
            "10",
            "--min-ops",
            "1",
            "--pilot",
            "2",
            "--seed",
            "77",
        ]));
        for name in ["a", "b", "c", "d", "e"] {
            bench.cell(name, "x", |m| Sample::ops(m as u64));
        }
        let report = bench.run_to(&mut out).unwrap();
        report
            .records
            .iter()
            .map(|r| (r.round, r.position, r.subject.clone()))
            .collect::<Vec<_>>()
    };
    let three = run("3");
    let five = run("5");
    assert_eq!(three.len(), 15);
    assert_eq!(five.len(), 25);
    assert_eq!(&five[..15], &three[..]);
    // and positions within a round are the recorded execution order
    for (i, (round, position, _)) in three.iter().enumerate() {
        assert_eq!(*round as usize, i / 5 + 1);
        assert_eq!(*position, i % 5);
    }
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
    // 1 ns per op: a million ops is 1 ms, well inside the budget
    assert_eq!(calibrate(1.0, &o), (1_000_000, Calibration::Full));
    // exactly at the ceiling still counts as Full
    assert_eq!(calibrate(2000.0, &o), (1_000_000, Calibration::Full));
    // 10 us per op: 2 s buys 200_000 ops
    assert_eq!(calibrate(10_000.0, &o), (200_000, Calibration::Budget));
    // 1 ms per op: 2 s buys 2_000, still above the floor
    assert_eq!(calibrate(1_000_000.0, &o), (2_000, Calibration::Budget));
    // 1 s per op: 2 s buys 2, below the floor of 1000
    assert_eq!(calibrate(1e9, &o), (1_000, Calibration::Floor));
    // a zero-cost pilot never divides by zero
    assert_eq!(calibrate(0.0, &o), (1_000_000, Calibration::Full));
    // the floor never exceeds the ceiling
    let small = options(&["--ops", "10", "--min-ops", "1000"]);
    assert_eq!(calibrate(1e9, &small), (10, Calibration::Floor));
}

#[test]
fn calibration_is_fixed_before_round_one_and_recorded() {
    bensho::row! { struct Tag { tag: &'static str, note: Option<u32> } }
    let mut out = Vec::new();
    let mut bench = Bench::<Tag>::new(options(&[
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
    ]));
    // slow: ~1 ms per requested op, so 20 ms buys ~20 ops, budget-calibrated
    bench.cell("slow", "m", |m| {
        std::thread::sleep(Duration::from_millis(m as u64));
        Sample::with(
            m as u64,
            Tag {
                tag: "s",
                note: None,
            },
        )
    });
    // fast: full ceiling
    bench.cell("fast", "m", |m| {
        Sample::with(
            m as u64 * 3,
            Tag {
                tag: "f",
                note: Some(1),
            },
        )
    });
    let report = bench.run_to(&mut out).unwrap();

    let slow: Vec<_> = report
        .records
        .iter()
        .filter(|r| r.subject == "slow")
        .collect();
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

    let fast: Vec<_> = report
        .records
        .iter()
        .filter(|r| r.subject == "fast")
        .collect();
    assert!(fast
        .iter()
        .all(|r| r.calibration == Calibration::Full && r.batch == 100_000));
    // ops is the closure's count, not the batch
    assert!(fast.iter().all(|r| r.ops == 300_000));

    let text = String::from_utf8(out).unwrap();
    let mut lines = text.lines();
    let header = lines.next().unwrap();
    let expected: Vec<&str> = COLUMNS.iter().copied().chain(["tag", "note"]).collect();
    assert_eq!(header, expected.join(","));
    let rows: Vec<&str> = lines.collect();
    assert_eq!(rows.len(), 6);
    for row in &rows {
        assert_eq!(row.split(',').count(), expected.len(), "{row}");
    }
    let fast_row = rows.iter().find(|r| r.starts_with("fast,m,")).unwrap();
    assert!(fast_row.ends_with(",f,1"), "{fast_row}");
    let slow_row = rows.iter().find(|r| r.starts_with("slow,m,")).unwrap();
    assert!(
        slow_row.ends_with(",Budget,") || slow_row.contains(",Budget,"),
        "{slow_row}"
    );
    assert!(
        slow_row.ends_with(",s,"),
        "None prints as the empty cell: {slow_row}"
    );
}

#[test]
fn csv_quotes_only_what_needs_it() {
    assert_eq!(bensho::quote("plain"), "plain");
    assert_eq!(bensho::quote("a,b"), "\"a,b\"");
    assert_eq!(bensho::quote("say \"hi\""), "\"say \"\"hi\"\"\"");
    assert_eq!(bensho::quote(""), "");
}

#[test]
fn colliding_user_column_is_refused_before_the_pilot() {
    bensho::row! { struct Bad { seed: u64 } }
    let mut out = Vec::new();
    let mut bench = Bench::<Bad>::new(options(&["--rounds", "1", "--ops", "1"]));
    let mut ran = false;
    bench.cell("a", "b", |m| {
        ran = true;
        Sample::with(m as u64, Bad { seed: 1 })
    });
    let err = bench.run_to(&mut out).unwrap_err();
    assert!(err.to_string().contains("seed"), "{err}");
    assert!(!ran);
}

#[test]
fn zero_ops_is_a_bench_bug() {
    let mut out = Vec::new();
    let mut bench = Bench::<()>::new(options(&["--rounds", "1", "--ops", "1"]));
    bench.cell("a", "b", |_| Sample::ops(0));
    let err = bench.run_to(&mut out).unwrap_err();
    assert!(err.to_string().contains("a/b"), "{err}");
}

#[test]
fn only_filters_at_registration() {
    let mut out = Vec::new();
    let mut bench = Bench::<()>::new(options(&[
        "--rounds", "1", "--ops", "1", "--only", "keep", "--only", "b/also",
    ]));
    bench.cell("keep", "x", |m| Sample::ops(m as u64));
    bench.cell("drop", "x", |m| Sample::ops(m as u64));
    bench.cell("b", "also", |m| Sample::ops(m as u64));
    let report = bench.run_to(&mut out).unwrap();
    let names: Vec<String> = report.records.iter().map(|r| r.subject.clone()).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"keep".to_string()) && names.contains(&"b".to_string()));
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
        "--no-header",
    ]);
    assert_eq!(o.seed, 255);
    assert_eq!(o.rounds, 2);
    assert!(!o.header);
    assert_eq!(o.rest, vec!["--large-mib", "8", "positional"]);
    assert!(Options::parse(["--rounds", "0"]).is_err());
    assert!(Options::parse(["--rounds"]).is_err());
    assert!(Options::parse(["--ops", "ten"]).is_err());
}

#[test]
fn own_elapsed_overrides_the_harness_stopwatch() {
    let mut out = Vec::new();
    let mut bench = Bench::<()>::new(options(&["--rounds", "1", "--ops", "4", "--pilot", "1"]));
    bench.cell("a", "b", |m| {
        std::thread::sleep(Duration::from_millis(5));
        Sample::ops(m as u64).timed(Duration::from_nanos(400))
    });
    let report = bench.run_to(&mut out).unwrap();
    assert_eq!(report.records[0].elapsed_ns, 400);
    assert_eq!(report.records[0].ns_per_op, 100.0);
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
