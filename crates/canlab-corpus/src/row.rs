//! One corpus row: the restricted TOML subset, its validation, and loading a
//! directory of rows.
//!
//! The subset is deliberately smaller than TOML. A row has top-level string and
//! number keys, one `expect` table (a `[expect]` header or dotted `expect.` keys)
//! and `#` comments. Everything else — arrays, booleans, dates, a second table —
//! is a load error, because a row that uses a feature the runner does not
//! understand would otherwise be read as something it is not.
//!
//! A load error is not a failure of the tool: it means the *row* is wrong, and
//! the run exits 2 rather than reporting a pass or a fail it cannot justify.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::document::{self, DocError};

/// Which half of the corpus a row belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    /// Mathematics.
    Math,
    /// Physics. The only subject that may carry a `unit`.
    Physics,
}

impl Subject {
    /// The directory name, which is also the value of the row's `subject` key.
    pub fn as_str(self) -> &'static str {
        match self {
            Subject::Math => "math",
            Subject::Physics => "physics",
        }
    }

    /// Read a subject from its directory name.
    pub fn parse(name: &str) -> Option<Subject> {
        match name {
            "math" => Some(Subject::Math),
            "physics" => Some(Subject::Physics),
            _ => None,
        }
    }
}

/// What a row expects the run to produce.
#[derive(Clone, Debug, PartialEq)]
pub enum Expect {
    /// The canonical LaTeX answer.
    Exact(String),
    /// A number, with the absolute tolerance that makes it right.
    Numeric {
        /// The expected value.
        value: f64,
        /// Absolute tolerance, in `unit`.
        tolerance: f64,
        /// Physics only. Recorded; the check waits for W15, so the row is
        /// reported `pass (unit unchecked)`.
        unit: Option<String>,
        /// Significant figures both sides are rounded to, half-even.
        digits: Option<u32>,
    },
    /// The reason the run must refuse for. `None` is not a form here: a refusal
    /// row always names the construct.
    Refused(String),
}

impl Expect {
    /// The expectation as one line, for the console and the report.
    pub fn describe(&self) -> String {
        match self {
            Expect::Exact(answer) => format!("exact {answer}"),
            Expect::Numeric {
                value,
                tolerance,
                unit,
                digits,
            } => {
                let mut out = format!("numeric {value} ± {tolerance}");
                if let Some(unit) = unit {
                    out.push(' ');
                    out.push_str(unit);
                }
                if let Some(digits) = digits {
                    out.push_str(&format!(" ({digits} s.f.)"));
                }
                out
            }
            Expect::Refused(reason) => format!("refused {reason:?}"),
        }
    }
}

/// One problem: the model, the answer and the citation.
#[derive(Clone, Debug)]
pub struct Row {
    /// Where the row lives. `tests/cases/<subject>/<id>.toml`.
    pub path: PathBuf,
    /// The file stem, unique across the corpus.
    pub id: String,
    /// `math` or `physics`, and equal to the directory.
    pub subject: Subject,
    /// The report's grouping tag.
    pub capability: String,
    /// The citation, never the statement.
    pub source: String,
    /// The student's LaTeX, as the editor serialises it.
    pub document: String,
    /// The expected outcome.
    pub expect: Expect,
    /// What the row tests and what it leaves out.
    pub note: Option<String>,
}

impl Row {
    /// Parse one row from TOML text.
    ///
    /// `path` is stored, not validated: [`load_row`] checks the layout, because
    /// only it knows what directory the text came from.
    pub fn parse(text: &str, path: &Path) -> Result<Row, String> {
        let parsed = parse_subset(text)?;

        let id = parsed
            .top
            .get("id")
            .ok_or("missing `id`")?
            .string("id")?;
        let subject_name = parsed
            .top
            .get("subject")
            .ok_or("missing `subject`")?
            .string("subject")?;
        let subject = Subject::parse(&subject_name)
            .ok_or_else(|| format!("`subject` must be `math` or `physics`, not {subject_name:?}"))?;
        let capability = parsed
            .top
            .get("capability")
            .ok_or("missing `capability`")?
            .string("capability")?;
        let source = parsed
            .top
            .get("source")
            .ok_or("missing `source`")?
            .string("source")?;
        let document = parsed
            .top
            .get("document")
            .ok_or("missing `document`")?
            .string("document")?;
        let note = match parsed.top.get("note") {
            Some(value) => Some(value.string("note")?),
            None => None,
        };

        if id.is_empty() {
            return Err(String::from("`id` must not be empty"));
        }
        if document.trim().is_empty() {
            return Err(String::from("`document` must not be empty"));
        }
        validate_capability(&capability)?;
        validate_source(&source)?;
        let expect = parse_expect(&parsed.expect, subject)?;

        let row = Row {
            path: path.to_path_buf(),
            id,
            subject,
            capability,
            source,
            document,
            expect,
            note,
        };
        row.check_document_shape()?;
        Ok(row)
    }

    /// `true` when the row cites an upstream dataset, which is what `hf:` does.
    pub fn is_stress(&self) -> bool {
        self.source.starts_with("hf:")
    }

    /// `true` when the row expects a refusal.
    pub fn is_refusal(&self) -> bool {
        matches!(self.expect, Expect::Refused(_))
    }

    /// The document as a question, or a description of why it is not one.
    ///
    /// A *construct* the editor refuses is not a load error: it is what a
    /// refusal row is for. Only a shape error — two unknowns in one equation, a
    /// system line that is not an equation, a definition that cannot be made —
    /// makes the row itself wrong.
    fn check_document_shape(&self) -> Result<(), String> {
        match document::compile(&self.document) {
            Ok(_) => Ok(()),
            Err(DocError::Load(reason)) => Err(reason),
            // The tool declines the construct. The run reports it; the row is
            // well-formed.
            Err(DocError::Refused(_)) => Ok(()),
        }
    }
}

/// A row that could not be loaded, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadError {
    /// The file.
    pub path: PathBuf,
    /// Why it is not a row.
    pub reason: String,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", path_text(&self.path), self.reason)
    }
}

/// A path as the corpus prints it: forward slashes on every platform, so a
/// report generated on Windows and one generated on Linux read the same.
pub fn path_text(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

/// Load and validate one row, checking the layout the format promises.
pub fn load_row(path: &Path) -> Result<Row, LoadError> {
    let fail = |reason: String| LoadError {
        path: path.to_path_buf(),
        reason,
    };

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| fail(String::from("the path has no file name")))?;
    let stem = file_name
        .strip_suffix(".toml")
        .ok_or_else(|| fail(String::from("a row must be a `.toml` file")))?;
    let subject_name = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .ok_or_else(|| fail(String::from("a row must live in a subject directory")))?;
    let subject = Subject::parse(subject_name).ok_or_else(|| {
        fail(format!(
            "the subject directory must be `math` or `physics`, not {subject_name:?}"
        ))
    })?;
    let cases = path
        .parent()
        .and_then(|parent| parent.parent())
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str());
    if cases != Some("cases") {
        return Err(fail(String::from(
            "a row must live at `tests/cases/<subject>/<id>.toml`",
        )));
    }

    let text = std::fs::read_to_string(path)
        .map_err(|error| fail(format!("could not read the file: {error}")))?;
    let row = Row::parse(&text, path).map_err(fail)?;

    if row.id != stem {
        return Err(fail(format!(
            "`id` is {:?} but the file name is {stem:?}",
            row.id
        )));
    }
    if row.subject != subject {
        return Err(fail(format!(
            "`subject` is {:?} but the directory is {subject_name:?}",
            row.subject.as_str()
        )));
    }
    Ok(row)
}

/// Load every row under `cases`, which is expected to be `tests/cases`.
///
/// A file outside a subject directory is an error, not something to skip: the
/// corpus is meant to be read whole. Duplicate ids are errors too — two rows with
/// one name would make every report lie about which file was measured.
pub fn load_rows(cases: &Path) -> (Vec<Row>, Vec<LoadError>) {
    let mut rows: Vec<Row> = Vec::new();
    let mut errors: Vec<LoadError> = Vec::new();

    let entries = match std::fs::read_dir(cases) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push(LoadError {
                path: cases.to_path_buf(),
                reason: format!("could not read the corpus directory: {error}"),
            });
            return (rows, errors);
        }
    };

    let mut subject_dir: Vec<PathBuf> = Vec::new();
    let mut stray: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subject_dir.push(path);
        } else if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            stray.push(path);
        }
    }
    subject_dir.sort();
    stray.sort();

    for path in stray {
        errors.push(LoadError {
            path: path.clone(),
            reason: String::from(
                "a row must live under a subject directory (`math` or `physics`), not at the top",
            ),
        });
    }

    for directory in subject_dir {
        let name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        if Subject::parse(&name).is_none() {
            errors.push(LoadError {
                path: directory.clone(),
                reason: format!(
                    "unknown subject directory {name:?}; a subject is `math` or `physics`"
                ),
            });
            continue;
        }
        let mut files: Vec<PathBuf> = match std::fs::read_dir(&directory) {
            Ok(entries) => entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("toml"))
                .collect(),
            Err(error) => {
                errors.push(LoadError {
                    path: directory.clone(),
                    reason: format!("could not read the subject directory: {error}"),
                });
                continue;
            }
        };
        files.sort();
        for file in files {
            match load_row(&file) {
                Ok(row) => rows.push(row),
                Err(error) => errors.push(error),
            }
        }
    }

    rows.sort_by(|a, b| a.path.cmp(&b.path));

    // A duplicate id is two files claiming one name. Keep neither in the run:
    // reporting one of them would make the other invisible.
    let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut duplicate: Vec<PathBuf> = Vec::new();
    for row in &rows {
        if let Some(first) = seen.get(&row.id) {
            errors.push(LoadError {
                path: row.path.clone(),
                reason: format!(
                    "duplicate id {:?}, already used by {}",
                    row.id,
                    first.display()
                ),
            });
            duplicate.push(row.path.clone());
        } else {
            seen.insert(row.id.clone(), row.path.clone());
        }
    }
    rows.retain(|row| !duplicate.contains(&row.path));

    (rows, errors)
}

/// `--tag X` takes a capability equal to `X` or starting `X.`.
pub fn matches_tag(capability: &str, tag: &str) -> bool {
    capability == tag
        || capability
            .strip_prefix(tag)
            .is_some_and(|rest| rest.starts_with('.'))
}

// ---------------------------------------------------------------------------
// The restricted TOML subset
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Value {
    String(String),
    Number(f64),
}

impl Value {
    fn string(&self, key: &str) -> Result<String, String> {
        match self {
            Value::String(text) => Ok(text.clone()),
            Value::Number(_) => Err(format!("`{key}` must be a string, not a number")),
        }
    }

    fn number(&self, key: &str) -> Result<f64, String> {
        match self {
            Value::Number(value) => Ok(*value),
            Value::String(_) => Err(format!("`{key}` must be a number, not a string")),
        }
    }

    fn integer(&self, key: &str) -> Result<i64, String> {
        let value = self.number(key)?;
        if value.fract() != 0.0 || !value.is_finite() {
            return Err(format!("`{key}` must be a whole number"));
        }
        Ok(value as i64)
    }
}

struct Parsed {
    top: BTreeMap<String, Value>,
    expect: BTreeMap<String, Value>,
}

fn parse_subset(text: &str) -> Result<Parsed, String> {
    #[derive(PartialEq)]
    enum Section {
        Top,
        Expect,
    }

    let mut section = Section::Top;
    let mut saw_expect_header = false;
    let mut top = BTreeMap::new();
    let mut expect = BTreeMap::new();

    for (number, raw) in text.lines().enumerate() {
        let line_number = number + 1;
        let line = strip_comment(raw);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(header) = line.strip_prefix('[') {
            let name = header
                .strip_suffix(']')
                .ok_or_else(|| format!("line {line_number}: an unclosed table header"))?
                .trim();
            if name != "expect" {
                return Err(format!(
                    "line {line_number}: `[{name}]` is outside the subset; only `[expect]` is a table"
                ));
            }
            if saw_expect_header {
                return Err(format!(
                    "line {line_number}: the `expect` table appears twice"
                ));
            }
            saw_expect_header = true;
            section = Section::Expect;
            continue;
        }

        let (key, value) = split_assignment(line, line_number)?;
        let value = parse_value(&value, line_number)?;

        match section {
            Section::Top => {
                if key == "expect" {
                    return Err(format!(
                        "line {line_number}: `expect` is a table; use `[expect]` or `expect.` keys"
                    ));
                }
                if let Some(sub) = key.strip_prefix("expect.") {
                    if sub.contains('.') {
                        return Err(format!(
                            "line {line_number}: `expect.{sub}` has more than one dot"
                        ));
                    }
                    insert_unique(&mut expect, sub, value, line_number)?;
                } else {
                    if key.contains('.') {
                        return Err(format!(
                            "line {line_number}: dotted key `{key}` is outside the subset"
                        ));
                    }
                    if !matches!(
                        key.as_str(),
                        "id" | "subject" | "capability" | "source" | "document" | "note"
                    ) {
                        return Err(format!(
                            "line {line_number}: `{key}` is not a field of a row"
                        ));
                    }
                    insert_unique(&mut top, &key, value, line_number)?;
                }
            }
            Section::Expect => {
                if let Some(sub) = key.strip_prefix("expect.") {
                    if sub.contains('.') {
                        return Err(format!(
                            "line {line_number}: `expect.{sub}` has more than one dot"
                        ));
                    }
                    insert_unique(&mut expect, sub, value, line_number)?;
                    continue;
                }
                if matches!(
                    key.as_str(),
                    "id" | "subject" | "capability" | "source" | "document" | "note"
                ) {
                    return Err(format!(
                        "line {line_number}: top-level `{key}` appears after the `[expect]` table"
                    ));
                }
                insert_unique(&mut expect, &key, value, line_number)?;
            }
        }
    }

    Ok(Parsed { top, expect })
}

fn insert_unique(
    map: &mut BTreeMap<String, Value>,
    key: &str,
    value: Value,
    line: usize,
) -> Result<(), String> {
    if map.insert(key.to_string(), value).is_some() {
        return Err(format!("line {line}: `{key}` appears twice"));
    }
    Ok(())
}

/// Everything before a `#` that is not inside a string.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        match quote {
            Some('"') => {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    quote = None;
                }
            }
            Some(_) => {
                if character == '\'' {
                    quote = None;
                }
            }
            None => match character {
                '\'' | '"' => quote = Some(character),
                '#' => return &line[..index],
                _ => {}
            },
        }
    }
    line
}

fn split_assignment(line: &str, line_number: usize) -> Result<(String, String), String> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        match quote {
            Some('"') => {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    quote = None;
                }
            }
            Some(_) => {
                if character == '\'' {
                    quote = None;
                }
            }
            None => match character {
                '\'' | '"' => quote = Some(character),
                '=' => {
                    let key = line[..index].trim();
                    let value = line[index + 1..].trim();
                    if key.is_empty() {
                        return Err(format!("line {line_number}: a key is missing before `=`"));
                    }
                    if value.is_empty() {
                        return Err(format!("line {line_number}: `{key}` has no value"));
                    }
                    return Ok((key.to_string(), value.to_string()));
                }
                _ => {}
            },
        }
    }
    Err(format!("line {line_number}: expected `key = value`"))
}

fn parse_value(text: &str, line_number: usize) -> Result<Value, String> {
    if let Some(rest) = text.strip_prefix('\'') {
        let end = rest
            .find('\'')
            .ok_or_else(|| format!("line {line_number}: an unterminated literal string"))?;
        if !rest[end + 1..].trim().is_empty() {
            return Err(format!(
                "line {line_number}: text after the closing quote of a literal string"
            ));
        }
        return Ok(Value::String(rest[..end].to_string()));
    }
    if let Some(rest) = text.strip_prefix('"') {
        let mut out = String::new();
        let mut characters = rest.char_indices();
        while let Some((index, character)) = characters.next() {
            match character {
                '"' => {
                    if !rest[index + 1..].trim().is_empty() {
                        return Err(format!(
                            "line {line_number}: text after the closing quote of a string"
                        ));
                    }
                    return Ok(Value::String(out));
                }
                '\\' => {
                    let (_, escape) = characters.next().ok_or_else(|| {
                        format!("line {line_number}: an unterminated escape sequence")
                    })?;
                    match escape {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '\\' => out.push('\\'),
                        '"' => out.push('"'),
                        other => {
                            return Err(format!(
                                "line {line_number}: `\\{other}` is not a supported escape; \
                                 use a literal string ('…') for LaTeX"
                            ));
                        }
                    }
                }
                other => out.push(other),
            }
        }
        return Err(format!("line {line_number}: an unterminated string"));
    }

    if text == "true" || text == "false" {
        return Err(format!(
            "line {line_number}: booleans are outside the subset; no field takes one"
        ));
    }
    if text.starts_with('[') || text.starts_with('{') {
        return Err(format!(
            "line {line_number}: arrays and inline tables are outside the subset"
        ));
    }

    let number = text.parse::<f64>().map_err(|_| {
        format!("line {line_number}: expected a string or a number, not {text:?}")
    })?;
    if !number.is_finite() {
        return Err(format!("line {line_number}: the number must be finite"));
    }
    Ok(Value::Number(number))
}

fn parse_expect(map: &BTreeMap<String, Value>, subject: Subject) -> Result<Expect, String> {
    for key in map.keys() {
        if !matches!(
            key.as_str(),
            "exact" | "numeric" | "tolerance" | "unit" | "digits" | "refused"
        ) {
            return Err(format!("`expect.{key}` is not a field of an expectation"));
        }
    }

    let forms: Vec<&str> = ["exact", "numeric", "refused"]
        .into_iter()
        .filter(|key| map.contains_key(*key))
        .collect();
    if forms.len() != 1 {
        return Err(match forms.len() {
            0 => String::from(
                "`expect` needs exactly one of `exact`, `numeric` or `refused`",
            ),
            _ => format!(
                "`expect` has more than one form: {}",
                forms.join(", ")
            ),
        });
    }
    let form = forms[0];

    for modifier in ["tolerance", "unit", "digits"] {
        if map.contains_key(modifier) && form != "numeric" {
            return Err(format!(
                "`expect.{modifier}` only belongs to a `numeric` expectation"
            ));
        }
    }

    match form {
        "exact" => {
            let answer = map["exact"].string("expect.exact")?;
            if answer.trim().is_empty() {
                return Err(String::from("`expect.exact` must not be empty"));
            }
            Ok(Expect::Exact(answer))
        }
        "refused" => {
            let reason = map["refused"].string("expect.refused")?;
            if reason.trim().is_empty() {
                return Err(String::from("`expect.refused` must name the construct"));
            }
            Ok(Expect::Refused(reason))
        }
        "numeric" => {
            let value = map["numeric"].number("expect.numeric")?;
            let tolerance = map
                .get("tolerance")
                .ok_or("a `numeric` expectation needs `expect.tolerance`")?
                .number("expect.tolerance")?;
            if tolerance < 0.0 {
                return Err(String::from("`expect.tolerance` must not be negative"));
            }
            let unit = match map.get("unit") {
                Some(value) => {
                    if subject != Subject::Physics {
                        return Err(String::from(
                            "`expect.unit` is physics only; a mathematics answer has no unit",
                        ));
                    }
                    let unit = value.string("expect.unit")?;
                    if unit.trim().is_empty() {
                        return Err(String::from("`expect.unit` must not be empty"));
                    }
                    Some(unit)
                }
                None => None,
            };
            let digits = match map.get("digits") {
                Some(value) => {
                    let digits = value.integer("expect.digits")?;
                    if !(1..=15).contains(&digits) {
                        return Err(String::from(
                            "`expect.digits` is a significant-figure count between 1 and 15",
                        ));
                    }
                    Some(digits as u32)
                }
                None => None,
            };
            Ok(Expect::Numeric {
                value,
                tolerance,
                unit,
                digits,
            })
        }
        _ => unreachable!("the form came from the fixed list above"),
    }
}

/// A capability is lowercase, dotted, and has an area and an operation.
fn validate_capability(capability: &str) -> Result<(), String> {
    let parts: Vec<&str> = capability.split('.').collect();
    if parts.len() < 2 {
        return Err(format!(
            "`capability` must be dotted as `area.operation`, not {capability:?}"
        ));
    }
    for part in &parts {
        if part.is_empty()
            || !part.starts_with(|c: char| c.is_ascii_lowercase())
            || !part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(format!(
                "`capability` must be lowercase, dotted, as `solve.quadratic`; \
                 {part:?} is not a segment"
            ));
        }
    }
    Ok(())
}

/// A source is a citation in one of the three documented forms.
fn validate_source(source: &str) -> Result<(), String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err(String::from("`source` must be a citation"));
    }
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        if trimmed.len() <= "https://".len() {
            return Err(String::from("`source` is a URL with nothing in it"));
        }
        return Ok(());
    }
    if let Some(rest) = trimmed.strip_prefix("hf:") {
        let (repo, tail) = rest
            .split_once(' ')
            .ok_or("an `hf:` source is `hf:<repo> <config>#<row-key>`")?;
        let (config, key) = tail
            .split_once('#')
            .ok_or("an `hf:` source needs `#<row-key>`")?;
        if repo.trim().is_empty() || !repo.contains('/') {
            return Err(String::from(
                "an `hf:` source needs a repository as `<owner>/<name>`",
            ));
        }
        if config.trim().is_empty() || key.trim().is_empty() {
            return Err(String::from(
                "an `hf:` source needs a config and a row key",
            ));
        }
        return Ok(());
    }
    if trimmed == "docs/corpus.md" || trimmed.starts_with("docs/corpus.md ") {
        return Ok(());
    }
    Err(format!(
        "`source` is a URL, an `hf:` id, or `docs/corpus.md §n`; not {trimmed:?}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_path(id: &str) -> PathBuf {
        PathBuf::from(format!("tests/cases/math/{id}.toml"))
    }

    const MINIMAL: &str = "\
id = \"a\"
subject = \"math\"
capability = \"solve.linear\"
source = \"https://example.test/paper — 2024, uppgift 1\"
document = 'x + 1 = 3'
expect.exact = '2'
";

    fn parse(text: &str) -> Result<Row, String> {
        Row::parse(text, &row_path("a"))
    }

    #[test]
    fn reads_the_five_fields_and_a_dotted_expectation() {
        let row = parse(MINIMAL).expect("valid row");
        assert_eq!(row.id, "a");
        assert_eq!(row.subject, Subject::Math);
        assert_eq!(row.capability, "solve.linear");
        assert_eq!(row.expect, Expect::Exact(String::from("2")));
        assert!(!row.is_stress());
        assert!(!row.is_refusal());
    }

    #[test]
    fn reads_the_expect_table_header() {
        let text = "\
id = \"a\"
subject = \"physics\"
capability = \"units.sigfigs\"
source = \"hf:x/y class#row-1\"
document = '2.5'
[expect]
numeric = 2.5
tolerance = 0.1
unit = \"m\"
digits = 2
";
        let row = parse(text).expect("valid row");
        assert_eq!(
            row.expect,
            Expect::Numeric {
                value: 2.5,
                tolerance: 0.1,
                unit: Some(String::from("m")),
                digits: Some(2),
            }
        );
        assert!(row.is_stress());
    }

    #[test]
    fn comments_do_not_eat_a_hash_inside_a_string() {
        let text = MINIMAL.replace(
            "source = \"https://example.test/paper — 2024, uppgift 1\"",
            "source = 'hf:owner/name config#Problem 2.26' # the hash is data",
        );
        let row = parse(&text).expect("valid row");
        assert_eq!(row.source, "hf:owner/name config#Problem 2.26");
    }

    #[test]
    fn a_literal_string_keeps_backslashes() {
        let text = MINIMAL.replace("document = 'x + 1 = 3'", "document = '\\frac{1}{2}'");
        let text = text.replace("expect.exact = '2'", "expect.exact = '\\frac{1}{2}'");
        let row = parse(&text).expect("valid row");
        assert_eq!(row.document, "\\frac{1}{2}");
        assert_eq!(row.expect, Expect::Exact(String::from("\\frac{1}{2}")));
    }

    #[test]
    fn the_subset_refuses_everything_it_does_not_understand() {
        let cases = [
            ("note = true", "booleans"),
            ("note = [\"a\"]", "arrays"),
            ("note = { a = 1 }", "inline tables"),
            ("[other]", "outside the subset"),
            ("note = 'a' # comment\n[expect]\nexact = '2'\n[expect]", "appears twice"),
            ("document = \"unterminated", "unterminated"),
            ("document = 'a' trailing", "text after the closing quote"),
            ("unknown = 'x'", "not a field of a row"),
            ("expect.exact.x = '1'", "more than one dot"),
            ("id = 'a'\nid = 'b'", "appears twice"),
        ];
        for (line, wanted) in cases {
            let text = format!("{MINIMAL}\n{line}\n");
            let error = parse(&text).expect_err(line);
            assert!(
                error.contains(wanted),
                "for {line:?} expected {wanted:?}, got {error:?}"
            );
        }
    }

    #[test]
    fn a_missing_or_wrong_field_is_a_load_error() {
        assert!(parse(&MINIMAL.replace("subject = \"math\"\n", "")).unwrap_err().contains("subject"));
        assert!(
            parse(&MINIMAL.replace("subject = \"math\"", "subject = \"chemistry\""))
                .unwrap_err()
                .contains("must be `math` or `physics`")
        );
        assert!(
            parse(&MINIMAL.replace("capability = \"solve.linear\"", "capability = \"Solve.quadratic\""))
                .unwrap_err()
                .contains("lowercase")
        );
        assert!(
            parse(&MINIMAL.replace("capability = \"solve.linear\"", "capability = \"linear\""))
                .unwrap_err()
                .contains("dotted")
        );
        assert!(
            parse(&MINIMAL.replace(
                "source = \"https://example.test/paper — 2024, uppgift 1\"",
                "source = \"a problem about ladders\""
            ))
            .unwrap_err()
            .contains("`source` is a URL")
        );
        assert!(
            parse(&MINIMAL.replace("expect.exact = '2'", "")).unwrap_err().contains("exactly one")
        );
    }

    #[test]
    fn an_expectation_has_exactly_one_form() {
        let both = format!("{MINIMAL}\nexpect.refused = 'division by zero'\n");
        assert!(parse(&both).unwrap_err().contains("more than one form"));

        let numeric_without_tolerance =
            MINIMAL.replace("expect.exact = '2'", "expect.numeric = 2");
        assert!(
            parse(&numeric_without_tolerance)
                .unwrap_err()
                .contains("needs `expect.tolerance`")
        );

        let unit_in_maths = MINIMAL.replace(
            "expect.exact = '2'",
            "expect.numeric = 2\nexpect.tolerance = 0.1\nexpect.unit = 'm'",
        );
        assert!(parse(&unit_in_maths).unwrap_err().contains("physics only"));

        let digits_zero = MINIMAL
            .replace("expect.exact = '2'", "expect.numeric = 2\nexpect.tolerance = 0.1\nexpect.digits = 0");
        assert!(parse(&digits_zero).unwrap_err().contains("between 1 and 15"));

        let negative = MINIMAL
            .replace("expect.exact = '2'", "expect.numeric = 2\nexpect.tolerance = -1");
        assert!(parse(&negative).unwrap_err().contains("negative"));
    }

    #[test]
    fn a_physics_numeric_row_may_carry_a_unit() {
        let text = MINIMAL
            .replace("subject = \"math\"", "subject = \"physics\"")
            .replace("expect.exact = '2'", "expect.numeric = 2\nexpect.tolerance = 0.5\nexpect.unit = 'm/s'");
        let row = parse(&text).expect("valid row");
        match row.expect {
            Expect::Numeric { unit, .. } => assert_eq!(unit.as_deref(), Some("m/s")),
            other => panic!("expected numeric, got {other:?}"),
        }
    }

    #[test]
    fn one_equation_with_two_unknowns_is_a_load_error() {
        let text = MINIMAL
            .replace("document = 'x + 1 = 3'", "document = 'x + y = 3'");
        let error = parse(&text).unwrap_err();
        assert!(error.contains("2 unknowns"), "got {error:?}");
    }

    #[test]
    fn a_construct_the_tool_declines_is_not_a_load_error() {
        // The row is well-formed even though the field cannot read the document:
        // that refusal is precisely what a `refused` row measures.
        let text = MINIMAL
            .replace("document = 'x + 1 = 3'", "document = '\\frac{1}{0}'")
            .replace("expect.exact = '2'", "expect.refused = '1/0'");
        let row = parse(&text).expect("a refused construct is a valid row");
        assert!(row.is_refusal());
    }

    #[test]
    fn a_system_is_solved_as_a_system_and_names_its_unknowns() {
        let text = MINIMAL
            .replace("document = 'x + 1 = 3'", "document = 'x + y = 3 || x - y = 1'")
            .replace("expect.exact = '2'", "expect.exact = 'x = 2; y = 1'");
        let row = parse(&text).expect("valid row");
        assert_eq!(row.document, "x + y = 3 || x - y = 1");
    }

    #[test]
    fn a_system_with_a_line_that_is_not_an_equation_is_a_load_error() {
        let text = MINIMAL
            .replace("document = 'x + 1 = 3'", "document = 'x + y = 3 || 2x'")
            .replace("expect.exact = '2'", "expect.exact = 'x = 2; y = 1'");
        let error = parse(&text).unwrap_err();
        assert!(error.contains("no `=`"), "got {error:?}");
    }

    #[test]
    fn tag_matching_takes_the_tag_and_its_children() {
        assert!(matches_tag("solve.quadratic", "solve"));
        assert!(matches_tag("solve.quadratic", "solve.quadratic"));
        assert!(!matches_tag("solve.quadratic", "solve.linear"));
        assert!(!matches_tag("solver.thing", "solve"));
        assert!(!matches_tag("solve", "solve.quadratic"));
    }
}
