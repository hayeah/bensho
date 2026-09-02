//! The toy bench from the spec: two ways of summing a slice, over two sizes,
//! with a `bytes` column on every row.
//!
//! ```text
//! cargo run --release --example toy -- --rounds 3 --out toy.csv
//! ```

use std::hint::black_box;

use bensho::{Bench, Options, Sample};

bensho::row! {
    /// Bytes read per op.
    pub struct ToyRow { bytes: u64 }
}

fn main() {
    let opts = Options::from_args();
    let small: Vec<u64> = (0..1_000).collect();
    let large: Vec<u64> = (0..1_000_000).collect();

    let mut bench = Bench::<ToyRow>::new(opts);
    for (mode, data) in [("small", &small), ("large", &large)] {
        let bytes = (data.len() * 8) as u64;
        bench.cell("vec_sum", mode, move |m| {
            let mut acc = 0u64;
            for _ in 0..m {
                acc = acc.wrapping_add(black_box(data).iter().sum::<u64>());
            }
            black_box(acc);
            Sample::with(m as u64, ToyRow { bytes })
        });
        bench.cell("fold_sum", mode, move |m| {
            let mut acc = 0u64;
            for _ in 0..m {
                acc =
                    acc.wrapping_add(black_box(data).iter().fold(0u64, |a, &x| a.wrapping_add(x)));
            }
            black_box(acc);
            Sample::with(m as u64, ToyRow { bytes })
        });
    }
    bench.run().expect("bensho run");
}
