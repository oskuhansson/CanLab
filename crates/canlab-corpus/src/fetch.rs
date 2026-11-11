//! Fetching the cited datasets, and checking `hf:` rows against them.
//!
//! The datasets are fetched, never vendored. `canlab corpus fetch [name]` puts
//! them under `%LOCALAPPDATA%\canlab\corpus\<dataset>\<revision>\`, with a
//! `manifest.toml` recording the URL, revision, byte count and SHA-256 of every
//! file it received. Rows point at the fetch by id; `tests/cases/` never holds
//! third-party data.
//!
//! There is no HTTP client in this workspace's dependency set. The transport is
//! `curl` — the one HTTPS client this machine already has — behind a trait, so
//! the whole fetch can be tested against a local directory with no network at
//! all. `CANLAB_CORPUS_MIRROR` selects that directory in the CLI.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::json::{self, Value};
use crate::row::{Expect, Row};
use crate::{compare, sha256};

/// One of the three cited datasets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dataset {
    /// OlympiadBench, read through the datasets-server JSON API because the
    /// Hugging Face copy is parquet.
    OlympiadBench,
    /// UGPhysics, JSONL.
    UgPhysics,
    /// SciBench, JSON.
    SciBench,
}

impl Dataset {
    /// Every dataset, in the order `fetch` takes them with no name.
    pub const ALL: [Dataset; 3] = [
        Dataset::OlympiadBench,
        Dataset::UgPhysics,
        Dataset::SciBench,
    ];

    /// The name the command takes, and the cache directory.
    pub fn name(self) -> &'static str {
        match self {
            Dataset::OlympiadBench => "olympiadbench",
            Dataset::UgPhysics => "ugphysics",
            Dataset::SciBench => "scibench",
        }
    }

    /// The Hugging Face repository.
    pub fn repo(self) -> &'static str {
        match self {
            Dataset::OlympiadBench => "Hothan/OlympiadBench",
            Dataset::UgPhysics => "UGPhysics/ugphysics",
            Dataset::SciBench => "xw27/scibench",
        }
    }

    /// The revision fetched. `main` is a revision, not a promise: the manifest
    /// records it, and `--update` re-fetches it rather than assuming it is the
    /// same content it was last time.
    pub fn revision(self) -> &'static str {
        "main"
    }

    /// Parse a dataset name.
    pub fn parse(name: &str) -> Option<Dataset> {
        Dataset::ALL
            .into_iter()
            .find(|dataset| dataset.name() == name)
    }

    /// The dataset whose repository is `repo`.
    pub fn from_repo(repo: &str) -> Option<Dataset> {
        Dataset::ALL
            .into_iter()
            .find(|dataset| dataset.repo() == repo)
    }

    /// The canonical URL of one file.
    pub fn file_url(self, revision: &str, path: &str) -> String {
        format!(
            "https://huggingface.co/datasets/{}/resolve/{revision}/{path}",
            self.repo()
        )
    }

    /// The canonical URL of one OlympiadBench page of rows.
    pub fn rows_url(revision: &str, config: &str) -> String {
        let _ = revision;
        format!(
            "https://datasets-server.huggingface.co/rows?dataset={}&config={config}&split=train&offset=0&length=100",
            Dataset::OlympiadBench.repo()
        )
    }

    /// The extension a file list is filtered by, for the two JSON datasets.
    fn extension(self) -> Option<&'static str> {
        match self {
            Dataset::OlympiadBench => None,
            Dataset::UgPhysics => Some("jsonl"),
            Dataset::SciBench => Some("json"),
        }
    }
}

/// An HTTPS client. The only implementation here shells out to `curl`; the
/// trait exists so a directory can stand in for the network in tests.
pub trait Transport {
    /// GET `url`.
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// HTTPS through the system `curl`.
pub struct CurlTransport {
    /// How long one request may take.
    pub timeout: Duration,
}

impl Default for CurlTransport {
    fn default() -> Self {
        CurlTransport {
            timeout: Duration::from_secs(60),
        }
    }
}

impl Transport for CurlTransport {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let seconds = self.timeout.as_secs().max(1).to_string();
        let output = Command::new("curl")
            .args(["-fsSL", "--max-time", &seconds, url])
            .output()
            .map_err(|error| {
                format!("network unavailable: could not start `curl`: {error}")
            })?;
        if output.status.success() {
            return Ok(output.stdout);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let line = stderr
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("curl failed");
        Err(format!("network unavailable: {line}"))
    }
}

/// Where a fetch reads from.
pub trait Source {
    /// Every file path in the repository, relative and `/`-separated.
    fn list(&self, dataset: Dataset, revision: &str) -> Result<Vec<String>, String>;
    /// One file's bytes.
    fn get(&self, dataset: Dataset, revision: &str, path: &str) -> Result<Vec<u8>, String>;
    /// The configs of OlympiadBench, for the datasets-server pages.
    fn olympiad_configs(&self, revision: &str) -> Result<Vec<String>, String>;
    /// One page of up to 100 OlympiadBench rows.
    fn olympiad_rows(&self, revision: &str, config: &str) -> Result<Vec<u8>, String>;
}

/// The real repository over HTTPS.
pub struct HttpSource {
    transport: Box<dyn Transport>,
}

impl HttpSource {
    /// Build from a transport.
    pub fn new(transport: Box<dyn Transport>) -> HttpSource {
        HttpSource { transport }
    }

    /// The default: HTTPS through `curl`.
    pub fn curl() -> HttpSource {
        HttpSource::new(Box::new(CurlTransport::default()))
    }
}

impl Source for HttpSource {
    fn list(&self, dataset: Dataset, revision: &str) -> Result<Vec<String>, String> {
        let url = format!(
            "https://huggingface.co/api/datasets/{}/tree/{revision}?recursive=true",
            dataset.repo()
        );
        let bytes = self.transport.get(&url)?;
        let text = String::from_utf8(bytes)
            .map_err(|error| format!("the file list is not UTF-8: {error}"))?;
        let value = json::parse(&text)
            .map_err(|error| format!("the file list is not JSON: {error}"))?;
        let files = value
            .as_array()
            .ok_or("the file list is not an array")?;
        let mut paths = Vec::new();
        for file in files {
            if file.get("type").and_then(Value::as_str) == Some("file")
                && let Some(path) = file.get("path").and_then(Value::as_str)
            {
                paths.push(path.to_string());
            }
        }
        paths.sort();
        Ok(paths)
    }

    fn get(&self, dataset: Dataset, revision: &str, path: &str) -> Result<Vec<u8>, String> {
        self.transport.get(&dataset.file_url(revision, path))
    }

    fn olympiad_configs(&self, _revision: &str) -> Result<Vec<String>, String> {
        let url = format!(
            "https://datasets-server.huggingface.co/splits?dataset={}",
            Dataset::OlympiadBench.repo()
        );
        let bytes = self.transport.get(&url)?;
        let text = String::from_utf8(bytes)
            .map_err(|error| format!("the config list is not UTF-8: {error}"))?;
        let value = json::parse(&text)
            .map_err(|error| format!("the config list is not JSON: {error}"))?;
        let splits = value
            .get("splits")
            .and_then(Value::as_array)
            .ok_or("the config list has no `splits` array")?;
        let mut configs: Vec<String> = Vec::new();
        for split in splits {
            if let Some(config) = split.get("config").and_then(Value::as_str)
                && !configs.iter().any(|known| known == config)
            {
                configs.push(config.to_string());
            }
        }
        configs.sort();
        Ok(configs)
    }

    fn olympiad_rows(&self, revision: &str, config: &str) -> Result<Vec<u8>, String> {
        self.transport
            .get(&Dataset::rows_url(revision, config))
    }
}

/// A local directory laid out like the datasets, for tests and offline mirrors.
pub struct LocalSource {
    root: PathBuf,
}

impl LocalSource {
    /// A source rooted at `root`, which holds `<dataset>/<path>` and
    /// `olympiadbench/rows/<config>.json`.
    pub fn new(root: impl Into<PathBuf>) -> LocalSource {
        LocalSource { root: root.into() }
    }

    fn dataset_root(&self, dataset: Dataset) -> PathBuf {
        self.root.join(dataset.name())
    }
}

impl Source for LocalSource {
    fn list(&self, dataset: Dataset, _revision: &str) -> Result<Vec<String>, String> {
        let root = self.dataset_root(dataset);
        if !root.is_dir() {
            return Err(format!(
                "the mirror has no `{}` directory at {}",
                dataset.name(),
                root.display()
            ));
        }
        let mut files = Vec::new();
        walk_files(&root, &root, &mut files)?;
        files.sort();
        Ok(files)
    }

    fn get(&self, dataset: Dataset, _revision: &str, path: &str) -> Result<Vec<u8>, String> {
        let file = self.dataset_root(dataset).join(path);
        std::fs::read(&file)
            .map_err(|error| format!("could not read mirror file {}: {error}", file.display()))
    }

    fn olympiad_configs(&self, _revision: &str) -> Result<Vec<String>, String> {
        let rows = self.root.join(Dataset::OlympiadBench.name()).join("rows");
        let entries = std::fs::read_dir(&rows).map_err(|error| {
            format!("could not read the OlympiadBench mirror at {}: {error}", rows.display())
        })?;
        let mut configs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("json")
                && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
            {
                configs.push(stem.to_string());
            }
        }
        configs.sort();
        Ok(configs)
    }

    fn olympiad_rows(&self, _revision: &str, config: &str) -> Result<Vec<u8>, String> {
        let file = self
            .root
            .join(Dataset::OlympiadBench.name())
            .join("rows")
            .join(format!("{config}.json"));
        std::fs::read(&file)
            .map_err(|error| format!("could not read mirror file {}: {error}", file.display()))
    }
}

fn walk_files(root: &Path, directory: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("could not read {}: {error}", directory.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files(root, &path, out)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            let relative = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push(relative);
        }
    }
    Ok(())
}

/// One file the fetch received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchedFile {
    /// The URL it came from (or would have, for a mirror).
    pub url: String,
    /// Its path inside the cache directory.
    pub path: String,
    /// How many bytes were received.
    pub bytes: usize,
    /// SHA-256 of those bytes, lowercase hex.
    pub sha256: String,
}

/// One dataset's fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchedDataset {
    /// Which dataset.
    pub dataset: Dataset,
    /// The revision recorded in the manifest.
    pub revision: String,
    /// `<cache>/<dataset>/<revision>`.
    pub directory: PathBuf,
    /// Every file, in the order it was written.
    pub files: Vec<FetchedFile>,
}

impl FetchedDataset {
    /// Total bytes received.
    pub fn bytes(&self) -> usize {
        self.files.iter().map(|file| file.bytes).sum()
    }
}

/// The cache root the CLI uses: `%LOCALAPPDATA%\canlab\corpus`, or the
/// `CANLAB_CORPUS_HOME` override, which is also how tests stay out of it.
pub fn default_cache_root() -> PathBuf {
    if let Some(home) = std::env::var_os("CANLAB_CORPUS_HOME") {
        return PathBuf::from(home);
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("canlab").join("corpus");
    }
    std::env::temp_dir().join("canlab-corpus")
}

/// Fetch the named datasets into `cache_root`.
pub fn fetch_with(
    source: &dyn Source,
    cache_root: &Path,
    datasets: &[Dataset],
    generated: &str,
) -> Result<Vec<FetchedDataset>, String> {
    let mut fetched = Vec::new();
    for dataset in datasets {
        fetched.push(fetch_one(source, cache_root, *dataset, generated)?);
    }
    Ok(fetched)
}

fn fetch_one(
    source: &dyn Source,
    cache_root: &Path,
    dataset: Dataset,
    generated: &str,
) -> Result<FetchedDataset, String> {
    let revision = dataset.revision();
    let directory = cache_root.join(dataset.name()).join(revision);

    // Everything is received before anything is written. A fetch that dies on
    // the seventh file must leave the cache as it was, or the next run would
    // score rows against a half-updated dataset and call it provenance.
    let mut received: Vec<(String, String, Vec<u8>)> = Vec::new();

    match dataset {
        Dataset::OlympiadBench => {
            let mut configs = source.olympiad_configs(revision)?;
            configs.sort();
            if configs.is_empty() {
                return Err(String::from(
                    "the OlympiadBench source listed no configs; nothing was fetched",
                ));
            }
            for config in configs {
                let bytes = source.olympiad_rows(revision, &config)?;
                let url = Dataset::rows_url(revision, &config);
                received.push((format!("rows/{config}.json"), url, bytes));
            }
        }
        _ => {
            let extension = dataset.extension().expect("the two JSON datasets have one");
            let mut paths: Vec<String> = source
                .list(dataset, revision)?
                .into_iter()
                .filter(|path| path.ends_with(&format!(".{extension}")))
                .collect();
            paths.sort();
            if paths.is_empty() {
                return Err(format!(
                    "the {} source listed no `.{extension}` files; nothing was fetched",
                    dataset.name()
                ));
            }
            for path in paths {
                let bytes = source.get(dataset, revision, &path)?;
                let url = dataset.file_url(revision, &path);
                received.push((path, url, bytes));
            }
        }
    }

    let mut files = Vec::new();
    for (path, url, bytes) in received {
        let target = directory.join(&path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        std::fs::write(&target, &bytes)
            .map_err(|error| format!("could not write {}: {error}", target.display()))?;
        files.push(FetchedFile {
            url,
            path,
            bytes: bytes.len(),
            sha256: sha256::hex(&bytes),
        });
    }

    let manifest = manifest_toml(dataset, revision, generated, &files);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    std::fs::write(directory.join("manifest.toml"), manifest)
        .map_err(|error| format!("could not write the manifest: {error}"))?;

    Ok(FetchedDataset {
        dataset,
        revision: revision.to_string(),
        directory,
        files,
    })
}

fn manifest_toml(
    dataset: Dataset,
    revision: &str,
    generated: &str,
    files: &[FetchedFile],
) -> String {
    let mut out = String::new();
    out.push_str("# Generated by `canlab corpus fetch`. Do not edit.\n");
    out.push_str(&format!("dataset = {}\n", toml_string(dataset.name())));
    out.push_str(&format!("revision = {}\n", toml_string(revision)));
    out.push_str(&format!("source = {}\n", toml_string(dataset.repo())));
    out.push_str(&format!("fetched = {}\n", toml_string(generated)));
    for file in files {
        out.push_str("\n[[files]]\n");
        out.push_str(&format!("url = {}\n", toml_string(&file.url)));
        out.push_str(&format!("path = {}\n", toml_string(&file.path)));
        out.push_str(&format!("bytes = {}\n", file.bytes));
        out.push_str(&format!("sha256 = {}\n", toml_string(&file.sha256)));
    }
    out
}

fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// --update
// ---------------------------------------------------------------------------

/// An `hf:` citation: repository, config and row key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HfSource {
    /// `owner/name`.
    pub repo: String,
    /// The dataset config.
    pub config: String,
    /// The row's key upstream.
    pub key: String,
}

/// Parse an `hf:<repo> <config>#<row-key>` citation.
pub fn parse_hf(source: &str) -> Option<HfSource> {
    let rest = source.strip_prefix("hf:")?;
    let (repo, tail) = rest.split_once(' ')?;
    let (config, key) = tail.split_once('#')?;
    if repo.is_empty() || config.is_empty() || key.is_empty() {
        return None;
    }
    Some(HfSource {
        repo: repo.to_string(),
        config: config.to_string(),
        key: key.to_string(),
    })
}

/// What `--update` found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateOutcome {
    /// How many `hf:` rows agreed with their upstream answer.
    pub checked: usize,
    /// Rows whose expected answer disagrees with upstream.
    pub drift: Vec<(PathBuf, String)>,
    /// Rows whose id is not in the fetched data.
    pub missing: Vec<(PathBuf, String)>,
}

impl UpdateOutcome {
    /// `true` when every cited row was checked and agreed.
    pub fn is_clean(&self) -> bool {
        self.drift.is_empty() && self.missing.is_empty()
    }
}

/// Re-fetch the cited datasets and check every `hf:` row against its upstream
/// answer. An expectation is never rewritten: drift is reported, not repaired.
pub fn check(
    rows: &[Row],
    source: &dyn Source,
    cache_root: &Path,
    generated: &str,
) -> Result<UpdateOutcome, String> {
    let mut datasets: Vec<Dataset> = Vec::new();
    for row in rows {
        let Some(hf) = parse_hf(&row.source) else {
            continue;
        };
        let dataset = Dataset::from_repo(&hf.repo).ok_or_else(|| {
            format!(
                "--update does not know the dataset `{}` cited by {}",
                hf.repo,
                row.path.display()
            )
        })?;
        if !datasets.contains(&dataset) {
            datasets.push(dataset);
        }
    }
    if datasets.is_empty() {
        return Ok(UpdateOutcome::default());
    }

    fetch_with(source, cache_root, &datasets, generated)?;

    let mut outcome = UpdateOutcome::default();
    for row in rows {
        let Some(hf) = parse_hf(&row.source) else {
            continue;
        };
        // A refusal row has no upstream answer to compare; it was not asked
        // to agree with a number.
        if matches!(&row.expect, Expect::Refused(_)) {
            continue;
        }
        {
            let dataset = Dataset::from_repo(&hf.repo)
                .expect("every cited repository was checked above");
            match upstream_answer(cache_root, dataset, &hf) {
                Ok(upstream) => {
                    if answer_agrees(&row.expect, &upstream) {
                        outcome.checked += 1;
                    } else {
                        outcome.drift.push((
                            row.path.clone(),
                            format!("upstream answer is {}", upstream.text),
                        ));
                    }
                }
                Err(reason) => outcome.missing.push((row.path.clone(), reason)),
            }
        };
    }
    Ok(outcome)
}

/// One upstream answer, as text and (when it is one) as a number.
#[derive(Clone, Debug)]
struct UpstreamAnswer {
    text: String,
    number: Option<f64>,
}

fn upstream_answer(
    cache_root: &Path,
    dataset: Dataset,
    hf: &HfSource,
) -> Result<UpstreamAnswer, String> {
    let directory = cache_root.join(dataset.name()).join(dataset.revision());
    let mut files = Vec::new();
    if directory.is_dir() {
        walk_files(&directory, &directory, &mut files)?;
    }
    // The config-named files first; a repository may hold several configs, and
    // the row's own config should win when both contain the key.
    let needle = hf.config.to_lowercase();
    files.sort_by_key(|path| {
        let lower = path.to_lowercase();
        (!lower.contains(&needle), lower.clone())
    });

    for file in files {
        if !(file.ends_with(".json") || file.ends_with(".jsonl")) {
            continue;
        }
        let path = directory.join(&file);
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let text = String::from_utf8(bytes)
            .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))?;
        let values: Vec<Value> = if file.ends_with(".jsonl") {
            text.lines()
                .filter(|line| !line.trim().is_empty())
                .filter_map(|line| json::parse(line).ok())
                .collect()
        } else {
            match json::parse(&text) {
                Ok(value) => vec![value],
                Err(_) => continue,
            }
        };
        for value in &values {
            if let Some(object) = find_row(value, &hf.key)
                && let Some(answer) = answer_of(object)
            {
                return Ok(answer);
            }
        }
    }
    Err(format!(
        "`{}` was not found in the fetched {} data",
        hf.key,
        dataset.name()
    ))
}

/// Find the object whose id column is `key`.
fn find_row<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    const KEYS: [&str; 6] = ["problemid", "problem_id", "id", "row_key", "key", "question_id"];
    if let Some(members) = value.as_object() {
        for name in KEYS {
            if let Some(Value::String(text)) = value.get(name)
                && text.as_str() == key
            {
                return Some(value);
            }
        }
        for (_, member) in members {
            if let Some(found) = find_row(member, key) {
                return Some(found);
            }
        }
    } else if let Some(items) = value.as_array() {
        for item in items {
            if let Some(found) = find_row(item, key) {
                return Some(found);
            }
        }
    }
    None
}

/// The comparable answer columns, in preference order.
fn answer_of(row: &Value) -> Option<UpstreamAnswer> {
    const COLUMNS: [&str; 7] = [
        "final_answer",
        "answer",
        "answers",
        "answer_latex",
        "answer_number",
        "answer_text",
        "solution",
    ];
    for column in COLUMNS {
        let Some(value) = row.get(column) else {
            continue;
        };
        let mut text = match value {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            Value::Array(items) => items
                .iter()
                .map(|item| match item {
                    Value::String(text) => text.clone(),
                    Value::Number(number) => number.to_string(),
                    _ => String::new(),
                })
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(", "),
            _ => continue,
        };
        text = strip_math_delimiters(&text);
        if text.is_empty() {
            continue;
        }
        let number = parse_number(&text);
        return Some(UpstreamAnswer { text, number });
    }
    None
}

fn strip_math_delimiters(text: &str) -> String {
    text.trim()
        .trim_matches('$')
        .trim()
        .trim_start_matches("\\(")
        .trim_end_matches("\\)")
        .trim_start_matches("\\[")
        .trim_end_matches("\\]")
        .trim()
        .to_string()
}

fn parse_number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().or_else(|| {
        text.split_whitespace()
            .next()
            .and_then(|first| first.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-').parse().ok())
    })
}

fn answer_agrees(expect: &Expect, upstream: &UpstreamAnswer) -> bool {
    match expect {
        Expect::Exact(expected) => compare::exact_matches(expected, &upstream.text),
        Expect::Numeric {
            value,
            tolerance,
            digits,
            ..
        } => upstream
            .number
            .map(|number| compare::numeric_matches(*value, number, *tolerance, *digits))
            .unwrap_or(false),
        Expect::Refused(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Subject;

    fn scratch(name: &str) -> PathBuf {
        // `CARGO_TARGET_TMPDIR` is only set for integration tests, not for the
        // unit tests this module runs in. The workspace `target/` is inside the
        // checkout, which is what the sandbox allows us to write to.
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/corpus-scratch");
        let path = base.join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch directory");
        path
    }

    fn mirror(root: &Path) {
        let write = |path: &str, text: &str| {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().expect("parent")).expect("directory");
            std::fs::write(file, text).expect("file");
        };
        write(
            "ugphysics/data/mechanics_en.jsonl",
            "{\"problem_id\":\"m-1\",\"answer\":\"4\"}\n",
        );
        write(
            "scibench/data/class.json",
            "[{\"problemid\":\"Problem 2.26\",\"answer_number\":2.3}]",
        );
        write(
            "olympiadbench/rows/algebra.json",
            "{\"rows\":[{\"row\":{\"id\":\"ob-1\",\"final_answer\":\"63\"}}]}",
        );
    }

    #[test]
    fn a_fetch_writes_the_files_and_a_manifest() {
        let scratch = scratch("fetch");
        let root = scratch.join("mirror");
        mirror(&root);
        let cache = scratch.join("cache");
        let fetched = fetch_with(
            &LocalSource::new(&root),
            &cache,
            &Dataset::ALL,
            "2026-11-11 12:00 UTC",
        )
        .expect("fetched");

        assert_eq!(fetched.len(), 3);
        let scibench = fetched
            .iter()
            .find(|dataset| dataset.dataset == Dataset::SciBench)
            .expect("scibench");
        assert_eq!(scibench.files.len(), 1);
        assert_eq!(scibench.files[0].path, "data/class.json");
        assert_eq!(
            scibench.files[0].url,
            "https://huggingface.co/datasets/xw27/scibench/resolve/main/data/class.json"
        );
        assert_eq!(scibench.files[0].bytes, 50);
        assert_eq!(scibench.files[0].sha256.len(), 64);

        let manifest = std::fs::read_to_string(scibench.directory.join("manifest.toml"))
            .expect("a manifest");
        assert!(manifest.contains("dataset = \"scibench\""));
        assert!(manifest.contains("revision = \"main\""));
        assert!(manifest.contains("sha256 = "));
        assert!(manifest.contains("bytes = 50"));
        assert!(manifest.contains("data/class.json"));
    }

    #[test]
    fn the_olympiad_sample_is_one_page_per_config() {
        let scratch = scratch("fetch-olympiad");
        let root = scratch.join("mirror");
        mirror(&root);
        let fetched = fetch_with(
            &LocalSource::new(&root),
            &scratch.join("cache"),
            &[Dataset::OlympiadBench],
            "now",
        )
        .expect("fetched");
        assert_eq!(fetched[0].files.len(), 1);
        assert_eq!(fetched[0].files[0].path, "rows/algebra.json");
        assert!(fetched[0].files[0].url.contains("length=100"));
        assert!(fetched[0].files[0].url.contains("config=algebra"));
    }

    #[test]
    fn a_mirror_with_no_files_is_an_error_not_an_empty_fetch() {
        let scratch = scratch("fetch-empty");
        std::fs::create_dir_all(scratch.join("mirror/ugphysics")).expect("mirror");
        let error = fetch_with(
            &LocalSource::new(scratch.join("mirror")),
            &scratch.join("cache"),
            &[Dataset::UgPhysics],
            "now",
        )
        .expect_err("must refuse");
        assert!(error.contains("no `.jsonl` files"), "{error}");
    }

    #[test]
    fn a_transport_failure_reads_as_network_unavailable() {
        struct Failing;
        impl Transport for Failing {
            fn get(&self, _url: &str) -> Result<Vec<u8>, String> {
                Err(String::from(
                    "network unavailable: the network is not reachable from here",
                ))
            }
        }
        let source = HttpSource::new(Box::new(Failing));
        let scratch = scratch("fetch-network");
        let error = fetch_with(&source, &scratch, &[Dataset::SciBench], "now")
            .expect_err("must fail");
        assert!(error.starts_with("network unavailable:"), "{error}");
        // The fetch failed before it wrote anything: the cache is as it was.
        assert!(
            !scratch.join("scibench").exists(),
            "a failed fetch left a cache directory behind"
        );
    }

    fn hf_row(key: &str, expect: Expect) -> Row {
        Row {
            path: PathBuf::from(format!("tests/cases/physics/{key}.toml")),
            id: key.to_string(),
            subject: Subject::Physics,
            capability: String::from("units.sigfigs"),
            source: format!("hf:xw27/scibench class#{key}"),
            document: String::from("2.3"),
            expect,
            note: None,
        }
    }

    #[test]
    fn update_checks_an_upstream_answer_and_reports_drift() {
        let scratch = scratch("update");
        let root = scratch.join("mirror");
        mirror(&root);
        let cache = scratch.join("cache");
        let source = LocalSource::new(&root);

        let agrees = update_one(
            &source,
            &cache,
            hf_row(
                "Problem 2.26",
                Expect::Numeric {
                    value: 2.3,
                    tolerance: 0.05,
                    unit: None,
                    digits: Some(2),
                },
            ),
        );
        assert_eq!(agrees.checked, 1);
        assert!(agrees.is_clean());

        let drifted = update_one(
            &source,
            &cache,
            hf_row(
                "Problem 2.26",
                Expect::Numeric {
                    value: 2.4,
                    tolerance: 0.001,
                    unit: None,
                    digits: None,
                },
            ),
        );
        assert_eq!(drifted.drift.len(), 1);
        assert!(drifted.drift[0].1.contains("2.3"), "{:?}", drifted.drift);
    }

    fn update_one(source: &dyn Source, cache: &Path, row: Row) -> UpdateOutcome {
        check(&[row], source, cache, "now").expect("checked")
    }

    #[test]
    fn a_missing_upstream_id_is_reported_not_guessed() {
        let scratch = scratch("update-missing");
        let root = scratch.join("mirror");
        mirror(&root);
        let outcome = update_one(
            &LocalSource::new(&root),
            &scratch.join("cache"),
            hf_row("Problem 9.99", Expect::Exact(String::from("2.3"))),
        );
        assert!(outcome.checked == 0);
        assert_eq!(outcome.missing.len(), 1);
        assert!(outcome.missing[0].1.contains("not found"));
    }

    #[test]
    fn an_exact_upstream_answer_matches_text() {
        let scratch = scratch("update-exact");
        let root = scratch.join("mirror");
        mirror(&root);
        let row = Row {
            path: PathBuf::from("tests/cases/math/ob-1.toml"),
            id: String::from("ob-1"),
            subject: Subject::Math,
            capability: String::from("calc.integral"),
            source: String::from("hf:Hothan/OlympiadBench algebra#ob-1"),
            document: String::from("63"),
            expect: Expect::Exact(String::from("63")),
            note: None,
        };
        let outcome = update_one(&LocalSource::new(&root), &scratch.join("cache"), row);
        assert_eq!(outcome.checked, 1);
    }

    #[test]
    fn an_unknown_repository_cannot_be_checked() {
        let mut row = hf_row("Problem 2.26", Expect::Exact(String::from("2.3")));
        row.source = String::from("hf:someone/else class#row");
        let scratch = scratch("update-unknown");
        let error = check(
            &[row],
            &LocalSource::new(scratch.join("mirror")),
            &scratch.join("cache"),
            "now",
        )
        .expect_err("must refuse");
        assert!(error.contains("does not know the dataset"), "{error}");
    }

    #[test]
    fn dataset_names_and_citations_round_trip() {
        for dataset in Dataset::ALL {
            assert_eq!(Dataset::parse(dataset.name()), Some(dataset));
            assert_eq!(Dataset::from_repo(dataset.repo()), Some(dataset));
        }
        let hf = parse_hf("hf:xw27/scibench class#Problem 2.26").expect("parsed");
        assert_eq!(hf.repo, "xw27/scibench");
        assert_eq!(hf.config, "class");
        assert_eq!(hf.key, "Problem 2.26");
        assert!(parse_hf("https://example.test").is_none());
    }
}
