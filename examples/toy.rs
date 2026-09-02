//! The toy bench from the spec: two ways of summing a slice, over two
//! sizes, each size a group whose state (the slice) is built per visit, plus
//! a singleton baseline cell.
//!
//! ```text
//! cargo run --release --example toy -- --rounds 3 --out out/
//! cargo run --release --example toy -- --list
//! cargo run --release --example toy -- --only toy/small/ --out out/
//! ```

use std::hint::black_box;

use bensho::{Group, Harness, Sample, Suite};

bensho::row! {
    /// Which summation, over how many bytes. `subject` repeats the cell
    /// name so the report can colour by it.
    pub struct ToyRow { subject: &'static str, bytes: u64 }
}

fn main() -> std::io::Result<()> {
    let harness = Harness::from_args();
    harness.suite("toy", |s: &mut Suite<ToyRow>| {
        for (size, len) in [("small", 1_000usize), ("large", 1_000_000)] {
            // The state: built when the round reaches this group, dropped
            // after its last cell. Never two of them alive at once.
            let setup = move || (0..len as u64).collect::<Vec<u64>>();
            s.group(size, setup, |g: &mut Group<ToyRow, Vec<u64>>| {
                g.cell("vec_sum", |data, m| {
                    let mut acc = 0u64;
                    for _ in 0..m {
                        acc = acc.wrapping_add(black_box(&*data).iter().sum::<u64>());
                    }
                    black_box(acc);
                    Sample::with(m as u64, row("vec_sum", data))
                });
                g.cell("fold_sum", |data, m| {
                    let mut acc = 0u64;
                    for _ in 0..m {
                        acc = acc.wrapping_add(
                            black_box(&*data)
                                .iter()
                                .fold(0u64, |a, &x| a.wrapping_add(x)),
                        );
                    }
                    black_box(acc);
                    Sample::with(m as u64, row("fold_sum", data))
                });
            });
        }
        // A singleton: a group of one with state `()`, no group directory.
        s.cell("baseline", |m| {
            for i in 0..m {
                black_box(i);
            }
            Sample::with(
                m as u64,
                ToyRow {
                    subject: "baseline",
                    bytes: 0,
                },
            )
        });
    })?;
    Ok(())
}

fn row(subject: &'static str, data: &[u64]) -> ToyRow {
    ToyRow {
        subject,
        bytes: (data.len() * 8) as u64,
    }
}
