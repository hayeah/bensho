//! The program-level object: the parsed options, the output directory, and
//! the one way a suite runs.

use std::cell::RefCell;
use std::io;

use crate::suite::check_name;
use crate::{Options, Report, Row, Suite};

/// The harness. One per process; `suite` runs each suite in turn.
pub struct Harness {
    options: Options,
    suites: RefCell<Vec<String>>,
}

impl Harness {
    pub fn new(options: Options) -> Harness {
        Harness {
            options,
            suites: RefCell::new(Vec::new()),
        }
    }

    /// `Options::from_args`, exiting with `USAGE` on error.
    pub fn from_args() -> Harness {
        Harness::new(Options::from_args())
    }

    pub fn options(&self) -> &Options {
        &self.options
    }

    /// Run one suite. When `--suite` admits it: `build` registers groups and
    /// cells, the directory and one file per cell are created, every group
    /// is calibrated, the rounds run, the summary goes to stderr and the
    /// report is returned. When it does not: `build` is not called, nothing
    /// on disk is touched, `Ok(None)`. Under `--list` the cells are printed
    /// and nothing runs; a suite left with no cells runs nothing and says so.
    pub fn suite<'a, R: Row + 'a>(
        &self,
        name: &str,
        build: impl FnOnce(&mut Suite<'a, R>),
    ) -> io::Result<Option<Report>> {
        check_name("suite", name)?;
        {
            let mut seen = self.suites.borrow_mut();
            if seen.iter().any(|s| s == name) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("suite {name:?} registered twice"),
                ));
            }
            seen.push(name.to_string());
        }
        if !self.options.enters(name) {
            eprintln!("-- {name}: skipped");
            return Ok(None);
        }
        let mut suite = Suite::new(name, self.options.clone());
        build(&mut suite);
        suite.check()?;
        if self.options.list {
            for path in suite.paths() {
                println!("{name}/{path}");
            }
            return Ok(None);
        }
        if suite.paths().is_empty() {
            eprintln!("-- {name}: no cells");
            return Ok(None);
        }
        let report = suite.run(&self.options.out_dir)?;
        eprint!("{}", report.summary());
        Ok(Some(report))
    }
}
