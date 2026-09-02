//! The run parameters and the CLI helper.

use std::path::PathBuf;
use std::time::Duration;

/// Everything the harness needs to know about a run. `parse` fills it from
/// argv; every argument it does not recognise lands in `rest`, in order, for
/// the bench's own flags.
#[derive(Clone, Debug)]
pub struct Options {
    /// Measured rounds after calibration.
    pub rounds: u32,
    /// The batch size ceiling, in requested ops.
    pub ops: usize,
    /// The batch size floor; a cell calibrated below it runs at the floor,
    /// flagged `Floor`.
    pub min_ops: usize,
    /// The pilot batch (the warm-up), capped at `ops`.
    pub pilot_ops: usize,
    /// The per-cell, per-round time the batch is fitted to.
    pub budget: Duration,
    /// The shuffle seed.
    pub seed: u64,
    /// Write the CSV header.
    pub header: bool,
    /// CSV destination; stdout when absent.
    pub out: Option<PathBuf>,
    /// Substring filters on `subject/mode`; a cell must match one of them
    /// when any are given.
    pub only: Vec<String>,
    /// Arguments bensho did not recognise, for the bench.
    pub rest: Vec<String>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            rounds: 5,
            ops: 1_000_000,
            min_ops: 1_000,
            pilot_ops: 1_024,
            budget: Duration::from_secs(2),
            seed: 0x5eed,
            header: true,
            out: None,
            only: Vec::new(),
            rest: Vec::new(),
        }
    }
}

pub const USAGE: &str = "bensho options:
  --rounds R        measured rounds after calibration (5)
  --ops M           batch size ceiling, requested ops per batch (1000000)
  --min-ops M       batch size floor (1000)
  --pilot P         pilot batch, doubles as warm-up (1024)
  --budget-ms MS    per-cell per-round time target the batch is fitted to (2000)
  --seed S          shuffle seed, decimal or 0x hex (0x5eed)
  --no-header       omit the CSV header (for concatenating runs)
  --out FILE        write CSV to FILE instead of stdout
  --only PATTERN    keep cells whose subject/mode contains PATTERN (repeatable)
anything else is left for the bench";

impl Options {
    /// Parse `args` (without the program name). Unknown arguments are not an
    /// error; they go to `rest`.
    pub fn parse<I>(args: I) -> Result<Options, String>
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let mut o = Options::default();
        let mut args = args.into_iter().map(Into::into);
        while let Some(a) = args.next() {
            let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} wants a value"));
            match a.as_str() {
                "--rounds" => o.rounds = number(&value("--rounds")?, "--rounds")? as u32,
                "--ops" => o.ops = number(&value("--ops")?, "--ops")? as usize,
                "--min-ops" => o.min_ops = number(&value("--min-ops")?, "--min-ops")? as usize,
                "--pilot" => o.pilot_ops = number(&value("--pilot")?, "--pilot")? as usize,
                "--budget-ms" => {
                    o.budget = Duration::from_millis(number(&value("--budget-ms")?, "--budget-ms")?)
                }
                "--seed" => o.seed = number(&value("--seed")?, "--seed")?,
                "--no-header" => o.header = false,
                "--out" => o.out = Some(PathBuf::from(value("--out")?)),
                "--only" => o.only.push(value("--only")?),
                _ => o.rest.push(a),
            }
        }
        if o.rounds == 0 {
            return Err("--rounds must be at least 1".into());
        }
        if o.ops == 0 || o.pilot_ops == 0 {
            return Err("--ops and --pilot must be at least 1".into());
        }
        if o.min_ops == 0 {
            o.min_ops = 1;
        }
        Ok(o)
    }

    /// `parse` over `std::env::args`, exiting with the usage on error.
    pub fn from_args() -> Options {
        match Options::parse(std::env::args().skip(1)) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("bensho: {e}\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    /// Whether a cell named `subject/mode` passes the `--only` filters.
    pub fn keeps(&self, subject: &str, mode: &str) -> bool {
        if self.only.is_empty() {
            return true;
        }
        let name = format!("{subject}/{mode}");
        self.only.iter().any(|p| name.contains(p.as_str()))
    }
}

fn number(s: &str, name: &str) -> Result<u64, String> {
    let parsed = match s.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => s.parse::<u64>(),
    };
    parsed.map_err(|_| format!("{name} wants a number, got {s:?}"))
}
