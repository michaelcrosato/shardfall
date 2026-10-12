//! Problems found in a definition, written for a model to read and fix: where (a path into
//! the document), what (a stable code and a message), the valid values, and a concrete fix.
//! A Rust port of SpawnForge's blueprint issues (`tools/creature-compiler/vendor/core/src/
//! blueprint/issues.ts` and `suggest.ts`); the vendored code is unchanged.

use std::fmt;

use serde::Serialize;

/// Readers report at most this many issues; the rest are summarised in one line.
pub const MAX_ISSUES: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// Stable machine-readable codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    UnknownKey,
    UnknownValue,
    UnknownPart,
    UnknownMaterial,
    UnknownAsset,
    Missing,
    InvalidType,
    OutOfRange,
    Conflict,
    Duplicate,
    Cycle,
    Limit,
    Format,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Issue {
    pub severity: Severity,
    /// Where, as written in the document: `parts.seat.shape.half[0]`, `ops[2].op`. "" is the
    /// root.
    pub path: String,
    pub code: Code,
    /// What is wrong, e.g. `-0.2 is outside 0.001-1000`.
    pub message: String,
    /// The valid range or values, when there are any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// A concrete suggested fix, e.g. `did you mean "color"?`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl Issue {
    pub fn error(path: impl Into<String>, code: Code, message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, path: path.into(), code, message: message.into(), expected: None, fix: None }
    }

    pub fn expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    pub fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    /// Adds a fix only when there is one.
    pub fn maybe_fix(mut self, fix: Option<String>) -> Self {
        if fix.is_some() {
            self.fix = fix;
        }
        self
    }
}

/// One line: `path: message (expected …) — fix`.
impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let path = if self.path.is_empty() { "(root)" } else { &self.path };
        write!(f, "{path}: {}", self.message)?;
        if let Some(expected) = self.expected.as_ref().filter(|e| !self.message.contains(e.as_str())) {
            write!(f, " (expected {expected})")?;
        }
        if let Some(fix) = &self.fix {
            write!(f, " — {fix}")?;
        }
        Ok(())
    }
}

/// Renders issues one per line, at most `MAX_ISSUES` of them.
pub fn render(issues: &[Issue]) -> String {
    let mut lines: Vec<String> = issues.iter().take(MAX_ISSUES).map(Issue::to_string).collect();
    if issues.len() > MAX_ISSUES {
        lines.push(format!("+{} more", issues.len() - MAX_ISSUES));
    }
    lines.join("\n")
}

/// `parent.key`, or `key` at the root.
pub fn join(parent: &str, key: &str) -> String {
    if parent.is_empty() { key.into() } else { format!("{parent}.{key}") }
}

/// Damerau–Levenshtein (optimal string alignment) distance.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev2 = vec![0; m + 1];
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(prev2[j - 2] + 1);
            }
            cur[j] = v;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

/// The closest candidate to `input`, if it is close enough to be a likely typo.
pub fn did_you_mean<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let lower = input.to_lowercase();
    let mut best = None;
    let mut best_score = f32::INFINITY;
    for candidate in candidates {
        let cl = candidate.to_lowercase();
        let mut score = edit_distance(&lower, &cl) as f32;
        // Dotted ids also match on a segment ("hron" → "horn.curved"), and a dotted guess on
        // its own segments.
        if cl.contains('.') {
            for segment in cl.split('.') {
                score = score.min(edit_distance(&lower, segment) as f32 + 0.5);
                if lower.contains('.') {
                    for own in lower.split('.').filter(|own| own.chars().count() >= 4) {
                        score = score.min(edit_distance(own, segment) as f32 + 1.0);
                    }
                }
            }
        }
        // An abbreviation ("len" → "length") is a stronger hint than a one-letter typo.
        if lower.chars().count() >= 3 && cl.starts_with(&lower) {
            score = score.min(0.5);
        }
        // Prefix or containment matches ("leg" → "foreleg") are good hints too.
        if score > 2.0 && (cl.starts_with(&lower) || lower.starts_with(&cl) || cl.contains(&lower)) {
            score = 2.0;
        }
        if score < best_score {
            best_score = score;
            best = Some(candidate);
        }
    }
    let limit = (input.chars().count() / 3).max(2) as f32;
    best.filter(|_| best_score <= limit)
}

/// `did you mean "x"?` for the closest candidate, if any.
pub fn suggestion<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    did_you_mean(input, candidates).map(|best| format!("did you mean \"{best}\"?"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_and_suggestions_match_spawnforge() {
        assert_eq!(edit_distance("colour", "color"), 1);
        assert_eq!(edit_distance("ab", "ba"), 1, "a transposition is one edit");
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(did_you_mean("colr", ["color", "look", "solid"]), Some("color"));
        assert_eq!(did_you_mean("len", ["length", "lean"]), Some("length"), "abbreviations win");
        assert_eq!(did_you_mean("hron", ["horn.curved", "tail.long"]), Some("horn.curved"));
        assert_eq!(did_you_mean("roof", ["roof_back", "rim"]), Some("roof_back"), "prefixes count");
        assert_eq!(did_you_mean("zzzzzz", ["color", "look"]), None);
    }

    #[test]
    fn issues_render_on_one_line() {
        let issue = Issue::error("ops[2] crate.fruit.count", Code::OutOfRange, "0 is outside 1-32")
            .expected("1-32")
            .fix("use a value in range");
        assert_eq!(issue.to_string(), "ops[2] crate.fruit.count: 0 is outside 1-32 — use a value in range");
        let root = Issue::error("", Code::Format, "not JSON").expected("an object");
        assert_eq!(root.to_string(), "(root): not JSON (expected an object)");
        let many: Vec<Issue> = (0..25).map(|i| Issue::error(format!("parts.p{i}"), Code::Missing, "missing")).collect();
        let text = render(&many);
        assert_eq!(text.lines().count(), MAX_ISSUES + 1);
        assert!(text.ends_with("+5 more"));
    }
}
