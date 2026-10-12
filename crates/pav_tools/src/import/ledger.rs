//! Ledgers: one row per take or model, tab separated, under a header of column names
//! (anim/cmu/takes.tsv; assets/catalogs/<lib>/<pack>.tsv). Lines starting with `#` explain the
//! columns. `csv_line` reads the comma-separated lists some libraries ship.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value};

/// The ledger of every CMU take (anim/cmu/takes.tsv): its frame rate, the stretch where it
/// moves, its category, fit and description.
#[derive(Default)]
pub struct Ledger {
    pub cols: Vec<String>,
    pub rows: Vec<Vec<String>>,
    by_id: HashMap<String, usize>,
}

impl Ledger {
    pub fn parse(text: &str) -> Ledger {
        let mut lines = text.lines().filter(|l| !l.is_empty() && !l.starts_with('#'));
        let cols: Vec<String> = lines.next().map(|h| h.split('\t').map(str::to_string).collect()).unwrap_or_default();
        let rows: Vec<Vec<String>> = lines.map(|l| l.split('\t').map(str::to_string).collect()).collect();
        let by_id = rows.iter().enumerate().filter_map(|(i, r)| r.first().map(|id| (id.clone(), i))).collect();
        Ledger { cols, rows, by_id }
    }

    pub fn read(path: &Path) -> Ledger {
        std::fs::read_to_string(path).map(|t| Ledger::parse(&t)).unwrap_or_default()
    }

    /// A take's value in a column.
    pub fn get(&self, id: &str, col: &str) -> Option<&str> {
        let c = self.cols.iter().position(|x| x == col)?;
        self.rows.get(*self.by_id.get(id)?)?.get(c).map(String::as_str)
    }

    pub fn row(&self, i: usize) -> Map<String, Value> {
        self.cols.iter().zip(&self.rows[i]).map(|(c, v)| (c.clone(), Value::from(v.as_str()))).collect()
    }
}

/// One line of a CSV file (quoted fields allowed).
pub fn csv_line(l: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = l.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|f| f.trim().to_string()).collect()
}
