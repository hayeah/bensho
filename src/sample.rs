//! What an op closure returns, and the stopwatch for time inside the engine.

use std::time::{Duration, Instant};

/// What an op closure returns: the count performed, optionally its own
/// timing, and the bench's row.
#[derive(Clone, Debug)]
pub struct Sample<R = ()> {
    /// Ops actually performed; the divisor of ns/op.
    pub ops: u64,
    /// `None`: the harness stopwatch around the call is the measurement.
    /// `Some`: the closure timed its own region.
    pub elapsed: Option<Duration>,
    pub row: R,
}

impl Sample<()> {
    pub fn ops(ops: u64) -> Sample<()> {
        Sample {
            ops,
            elapsed: None,
            row: (),
        }
    }
}

impl<R> Sample<R> {
    pub fn with(ops: u64, row: R) -> Sample<R> {
        Sample {
            ops,
            elapsed: None,
            row,
        }
    }

    pub fn timed(mut self, elapsed: Duration) -> Sample<R> {
        self.elapsed = Some(elapsed);
        self
    }
}

/// An accumulator for "time inside the engine": wrap each engine call in
/// `time`, read `ns` at the end of the batch, put it in a row column.
#[derive(Clone, Debug, Default)]
pub struct Stopwatch {
    ns: u128,
}

impl Stopwatch {
    pub fn new() -> Stopwatch {
        Stopwatch::default()
    }

    pub fn time<T>(&mut self, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let out = f();
        self.ns += start.elapsed().as_nanos();
        out
    }

    pub fn ns(&self) -> u128 {
        self.ns
    }

    pub fn reset(&mut self) {
        self.ns = 0;
    }

    /// Read and reset.
    pub fn take(&mut self) -> u128 {
        std::mem::take(&mut self.ns)
    }
}
