//! A suite: one row type, groups of cells over a state built per visit, the
//! calibration pass and the shuffled rounds.

use std::collections::BTreeSet;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::csv::CellFile;
use crate::{
    calibrate, cell_path, schedule, schedule_in_group, Calibrated, Calibration, Options, Record,
    Report, Row, Sample,
};

/// A suite, group or cell name: relative, no empty or `..` components, not
/// spelled with `.csv`.
pub fn check_name(kind: &str, name: &str) -> io::Result<()> {
    let bad = |why: &str| {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{kind} name {name:?} {why}"),
        ))
    };
    if name.is_empty() {
        return bad("is empty");
    }
    if name.starts_with('/') {
        return bad("is absolute");
    }
    if name.ends_with(".csv") {
        return bad("ends in .csv");
    }
    for component in name.split('/') {
        match component {
            "" => return bad("has an empty component"),
            "." | ".." => return bad("has a . or .. component"),
            _ => {}
        }
    }
    Ok(())
}

fn duplicate(kind: &str, name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{kind} {name:?} registered twice"),
    )
}

/// What a group does when the schedule reaches it, with its state type
/// erased: build the state, run one cell against it, drop it.
trait Visit<R> {
    fn enter(&mut self);
    fn leave(&mut self);
    fn run(&mut self, cell: usize, batch: usize) -> Sample<R>;
}

type Op<'a, R, S> = Box<dyn FnMut(&mut S, usize) -> Sample<R> + 'a>;

struct Erased<'a, R, S> {
    setup: Box<dyn FnMut() -> S + 'a>,
    ops: Vec<Op<'a, R, S>>,
    state: Option<S>,
}

impl<R, S> Visit<R> for Erased<'_, R, S> {
    fn enter(&mut self) {
        self.state = Some((self.setup)());
    }

    fn leave(&mut self) {
        self.state = None;
    }

    fn run(&mut self, cell: usize, batch: usize) -> Sample<R> {
        let state = self
            .state
            .as_mut()
            .expect("a group's state is built before its cells run");
        (self.ops[cell])(state, batch)
    }
}

struct CellMeta {
    name: String,
    batch: usize,
    calibration: Calibration,
    pilot_ns_per_op: f64,
}

struct GroupEntry<'a, R> {
    /// Empty for a singleton cell.
    name: String,
    cells: Vec<CellMeta>,
    visit: Box<dyn Visit<R> + 'a>,
}

/// A group under construction: the cells that share one state. Only
/// `Suite::group` makes one.
pub struct Group<'a, R, S> {
    name: String,
    suite: String,
    options: Options,
    cells: Vec<(String, Op<'a, R, S>)>,
    error: Option<io::Error>,
}

impl<'a, R, S> Group<'a, R, S> {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Register a cell. `op` receives the group's state and the requested
    /// batch size and returns the count it performed; its body is the timed
    /// region. Cells the filters exclude are dropped here.
    pub fn cell(
        &mut self,
        name: impl Into<String>,
        op: impl FnMut(&mut S, usize) -> Sample<R> + 'a,
    ) -> &mut Self {
        let name = name.into();
        if let Err(e) = check_name("cell", &name) {
            self.error.get_or_insert(e);
            return self;
        }
        if self
            .options
            .keeps(&self.suite, &cell_path(&self.name, &name))
        {
            self.cells.push((name, Box::new(op)));
        }
        self
    }
}

/// A suite under construction: one row type, groups and singleton cells.
/// Only `Harness::suite` makes one.
pub struct Suite<'a, R: Row + 'a> {
    name: String,
    options: Options,
    groups: Vec<GroupEntry<'a, R>>,
    paths: BTreeSet<String>,
    error: Option<io::Error>,
}

impl<'a, R: Row + 'a> Suite<'a, R> {
    pub(crate) fn new(name: &str, options: Options) -> Suite<'a, R> {
        Suite {
            name: name.to_string(),
            options,
            groups: Vec::new(),
            paths: BTreeSet::new(),
            error: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn options(&self) -> &Options {
        &self.options
    }

    /// The run parameters for this suite alone; the CLI values are what it
    /// starts from.
    pub fn options_mut(&mut self) -> &mut Options {
        &mut self.options
    }

    /// Register a singleton cell: a group of one with state `()`. `op`
    /// receives the requested batch size and returns the count it performed.
    pub fn cell(
        &mut self,
        name: impl Into<String>,
        mut op: impl FnMut(usize) -> Sample<R> + 'a,
    ) -> &mut Self {
        let name = name.into();
        let mut group = self.open_group(String::new());
        group.cell(name, move |_: &mut (), m| op(m));
        self.close_group(group, Box::new(|| ()));
        self
    }

    /// Register a group: `setup` builds its state each time the schedule
    /// reaches it, `build` registers the cells that use that state.
    pub fn group<S: 'a>(
        &mut self,
        name: impl Into<String>,
        setup: impl FnMut() -> S + 'a,
        build: impl FnOnce(&mut Group<'a, R, S>),
    ) -> &mut Self {
        let name = name.into();
        if let Err(e) = check_name("group", &name) {
            self.error.get_or_insert(e);
            return self;
        }
        let mut group = self.open_group(name);
        build(&mut group);
        self.close_group(group, Box::new(setup));
        self
    }

    fn open_group<S>(&self, name: String) -> Group<'a, R, S> {
        Group {
            name,
            suite: self.name.clone(),
            options: self.options.clone(),
            cells: Vec::new(),
            error: None,
        }
    }

    fn close_group<S: 'a>(&mut self, group: Group<'a, R, S>, setup: Box<dyn FnMut() -> S + 'a>) {
        if let Some(e) = group.error {
            self.error.get_or_insert(e);
        }
        if group.cells.is_empty() {
            return;
        }
        let mut cells = Vec::new();
        let mut ops = Vec::new();
        for (name, op) in group.cells {
            let path = cell_path(&group.name, &name);
            if !self.paths.insert(path.clone()) {
                self.error.get_or_insert(duplicate("cell", &path));
            }
            cells.push(CellMeta {
                name,
                batch: 0,
                calibration: Calibration::Full,
                pilot_ns_per_op: 0.0,
            });
            ops.push(op);
        }
        self.groups.push(GroupEntry {
            name: group.name,
            cells,
            visit: Box::new(Erased {
                setup,
                ops,
                state: None,
            }),
        });
    }

    /// The first registration error, if any.
    pub(crate) fn check(&mut self) -> io::Result<()> {
        match self.error.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Every cell path that would run, in registration order.
    pub(crate) fn paths(&self) -> Vec<String> {
        self.groups
            .iter()
            .flat_map(|g| g.cells.iter().map(move |c| cell_path(&g.name, &c.name)))
            .collect()
    }

    /// Create the files, calibrate every group, run the rounds.
    pub(crate) fn run(mut self, out_dir: &Path) -> io::Result<Report> {
        let dir = out_dir.join(&self.name);
        let mut files: Vec<Vec<CellFile>> = Vec::new();
        for g in &self.groups {
            let mut group_files = Vec::new();
            for c in &g.cells {
                let path = dir.join(format!("{}.csv", cell_path(&g.name, &c.name)));
                group_files.push(CellFile::create(path, R::columns())?);
            }
            files.push(group_files);
        }

        let mut report = Report {
            suite: self.name.clone(),
            ..Report::default()
        };
        let options = self.options.clone();
        let suite = self.name.clone();

        for g in &mut self.groups {
            g.visit.enter();
            for ci in 0..g.cells.len() {
                let pilot = options.pilot_ops.min(options.ops);
                let (sample, elapsed) = measure(&suite, g, ci, pilot)?;
                let per_request = elapsed.as_nanos() as f64 / pilot as f64;
                let (batch, calibration) = calibrate(per_request, &options);
                let cell = &mut g.cells[ci];
                cell.batch = batch;
                cell.calibration = calibration;
                cell.pilot_ns_per_op = elapsed.as_nanos() as f64 / sample.ops as f64;
                report.calibrations.push(Calibrated {
                    group: g.name.clone(),
                    name: cell.name.clone(),
                    batch,
                    calibration,
                    pilot_ns_per_op: cell.pilot_ns_per_op,
                });
            }
            g.visit.leave();
        }

        let cells: usize = self.groups.iter().map(|g| g.cells.len()).sum();
        let seed = options.seed;
        let run_start = Instant::now();
        for round in 1..=options.rounds {
            let mut position = 0;
            let order = schedule(seed, round, self.groups.len());
            for (group_position, gi) in order.into_iter().enumerate() {
                let g = &mut self.groups[gi];
                g.visit.enter();
                for ci in schedule_in_group(seed, round, gi, g.cells.len()) {
                    let start_ms = run_start.elapsed().as_millis();
                    let batch = g.cells[ci].batch;
                    let (sample, elapsed) = measure(&suite, g, ci, batch)?;
                    let cell = &g.cells[ci];
                    let record = Record {
                        suite: suite.clone(),
                        group: g.name.clone(),
                        name: cell.name.clone(),
                        round,
                        position,
                        group_position,
                        cells,
                        seed,
                        batch,
                        ops: sample.ops,
                        elapsed_ns: elapsed.as_nanos(),
                        ns_per_op: elapsed.as_nanos() as f64 / sample.ops as f64,
                        calibration: cell.calibration,
                        pilot_ns_per_op: cell.pilot_ns_per_op,
                        start_ms,
                    };
                    files[gi][ci].append(&record, &sample.row)?;
                    report.records.push(record);
                    position += 1;
                }
                g.visit.leave();
            }
        }
        Ok(report)
    }
}

/// Run one batch under the harness stopwatch; the closure's own `elapsed`
/// wins when it reports one.
fn measure<R>(
    suite: &str,
    g: &mut GroupEntry<'_, R>,
    cell: usize,
    batch: usize,
) -> io::Result<(Sample<R>, Duration)> {
    let start = Instant::now();
    let sample = g.visit.run(cell, batch);
    let harness = start.elapsed();
    if sample.ops == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "cell {}/{} reported zero ops for a batch of {batch}",
                suite,
                cell_path(&g.name, &g.cells[cell].name)
            ),
        ));
    }
    let elapsed = sample.elapsed.unwrap_or(harness);
    Ok((sample, elapsed))
}
