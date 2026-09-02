//! One CSV file per cell: header on create, open-append-close per row,
//! RFC-4180 quoting only where a value needs it.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use crate::{Record, Row, COLUMNS};

/// A cell's file. Holds the path, never a handle.
pub(crate) struct CellFile {
    path: PathBuf,
    user_columns: usize,
}

impl CellFile {
    /// Create the parent directories and the file, truncating, and write the
    /// header: bensho's columns, then `data.<field>` per user column.
    pub(crate) fn create(path: PathBuf, user_columns: &[&str]) -> io::Result<CellFile> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut fields: Vec<String> = COLUMNS.iter().map(|c| c.to_string()).collect();
        fields.extend(user_columns.iter().map(|c| format!("data.{c}")));
        let mut file = fs::File::create(&path)?;
        writeln!(file, "{}", line(&fields))?;
        Ok(CellFile {
            path,
            user_columns: user_columns.len(),
        })
    }

    /// Append one record and its row, opening and closing the file.
    pub(crate) fn append<R: Row>(&self, rec: &Record, row: &R) -> io::Result<()> {
        let values = row.values();
        if values.len() != self.user_columns {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "row for {}/{} has {} values for {} columns",
                    rec.suite,
                    rec.path(),
                    values.len(),
                    self.user_columns
                ),
            ));
        }
        let mut fields = rec.values();
        fields.extend(values);
        let mut file = OpenOptions::new().append(true).open(&self.path)?;
        writeln!(file, "{}", line(&fields))
    }
}

fn line(fields: &[String]) -> String {
    let quoted: Vec<String> = fields.iter().map(|f| quote(f)).collect();
    quoted.join(",")
}

/// RFC-4180 quoting, applied only when the value needs it.
pub fn quote(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
