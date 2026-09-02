//! The CSV writer: bensho's columns, then the row's, RFC-4180 quoting only
//! where a value needs it.

use std::io::{self, Write};

use crate::{Record, Row, COLUMNS};

pub struct CSVWriter<'w> {
    out: &'w mut dyn Write,
    user_columns: &'static [&'static str],
}

impl<'w> CSVWriter<'w> {
    /// Check the user's columns against bensho's and write the header if
    /// asked. A user column named like a bensho column is a bench bug.
    pub fn new(
        out: &'w mut dyn Write,
        user_columns: &'static [&'static str],
        header: bool,
    ) -> io::Result<CSVWriter<'w>> {
        for c in user_columns {
            if COLUMNS.contains(c) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("row column {c:?} collides with a bensho column"),
                ));
            }
        }
        let mut w = CSVWriter { out, user_columns };
        if header {
            let mut fields: Vec<String> = COLUMNS.iter().map(|c| c.to_string()).collect();
            fields.extend(user_columns.iter().map(|c| c.to_string()));
            w.line(&fields)?;
        }
        Ok(w)
    }

    pub fn record<R: Row>(&mut self, rec: &Record, row: &R) -> io::Result<()> {
        let values = row.values();
        if values.len() != self.user_columns.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "row for {}/{} has {} values for {} columns",
                    rec.subject,
                    rec.mode,
                    values.len(),
                    self.user_columns.len()
                ),
            ));
        }
        let mut fields = rec.values();
        fields.extend(values);
        self.line(&fields)?;
        self.out.flush()
    }

    fn line(&mut self, fields: &[String]) -> io::Result<()> {
        let line: Vec<String> = fields.iter().map(|f| quote(f)).collect();
        writeln!(self.out, "{}", line.join(","))
    }
}

/// RFC-4180 quoting, applied only when the value needs it.
pub fn quote(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
