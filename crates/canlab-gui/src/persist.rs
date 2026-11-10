//! What the app remembers between runs: the document and the look.
//!
//! This is a small, hand-written, versioned text format rather than `serde`
//! because the payload is six fields and one of them is a list of strings. A
//! dependency would have to be pinned, vendored and kept compatible for that; a
//! format with a version number on the first line is smaller than the
//! `Cargo.toml` entry it replaces, and it is readable when something goes wrong.
//!
//! # The format
//!
//! ```text
//! canlab 1
//! palette Solarized Dark
//! theme dark
//! accent 255 138 96
//! range -10 10
//! axes x y
//! parameter a 1.5
//! system
//! line \frac{x^2+1}{x}
//! line
//! system
//! line x+y=3
//! ```
//!
//! One directive per line; `system` starts a new group and `line` adds one to the
//! current group. An empty `line` is an empty line, which is a real state of the
//! editor. `theme` and `accent` are written for builds that predate palettes and
//! are read when `palette` is absent; `palette` decides. Anything unrecognised is
//! ignored, and anything malformed makes the whole file unreadable rather than
//! half-loading a document: a session file that silently loses the second half of
//! a page is worse than one that is ignored.
//!
//! # Versioning
//!
//! The first line is `canlab <version>`. A file from a *newer* version is refused,
//! because guessing at fields this build has never heard of is how a save becomes
//! data loss. An older one is read with the defaults for whatever is missing.

use std::path::{Path, PathBuf};

/// The format version this build writes and the newest it reads.
pub const VERSION: u32 = 1;

/// One saved session.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// The document: systems, each a list of LaTeX lines.
    pub document: Vec<Vec<String>>,
    /// The palette, by name.
    pub palette: String,
    /// Dark or light. Written for older builds; the palette decides.
    pub dark: bool,
    /// The accent colour. Written for older builds; the palette decides.
    pub accent: [u8; 3],
    /// The plot window.
    pub x_range: (f64, f64),
    /// The variables on the horizontal and vertical axes.
    pub axes: (Option<String>, Option<String>),
    /// Values given to the variables that are not on an axis.
    pub parameters: Vec<(String, f64)>,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            document: vec![vec![String::new()]],
            palette: String::from("Midnight"),
            dark: true,
            accent: [0xE8, 0x7D, 0x0D],
            x_range: (-10.0, 10.0),
            axes: (None, None),
            parameters: Vec::new(),
        }
    }
}

impl Session {
    /// Render the session as the text format.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("canlab {VERSION}\n"));
        if !self.palette.is_empty() {
            out.push_str(&format!("palette {}\n", self.palette));
        }
        out.push_str(if self.dark { "theme dark\n" } else { "theme light\n" });
        out.push_str(&format!(
            "accent {} {} {}\n",
            self.accent[0], self.accent[1], self.accent[2]
        ));
        out.push_str(&format!(
            "range {} {}\n",
            number(self.x_range.0),
            number(self.x_range.1)
        ));
        out.push_str(&format!(
            "axes {} {}\n",
            self.axes.0.as_deref().unwrap_or("-"),
            self.axes.1.as_deref().unwrap_or("-")
        ));
        for (name, value) in &self.parameters {
            // A parameter name that needed escaping would not be a symbol the
            // editor could have made, so spaces are refused at the source.
            if name.contains(char::is_whitespace) {
                continue;
            }
            out.push_str(&format!("parameter {name} {}\n", number(*value)));
        }
        for system in &self.document {
            out.push_str("system\n");
            for line in system {
                out.push_str("line ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    /// Read the text format, or explain why not.
    pub fn from_text(text: &str) -> Result<Session, String> {
        let mut session = Session {
            document: Vec::new(),
            // Empty until a `palette` line is seen: a file from before palettes
            // has a light/dark flag and one accent instead, and the caller maps
            // that onto a scheme. Defaulting to a name here would silently
            // override the older field.
            palette: String::new(),
            ..Session::default()
        };
        let mut lines = text.lines();
        let header = lines.next().unwrap_or_default().trim();
        let Some(rest) = header.strip_prefix("canlab") else {
            return Err(String::from("this is not a canlab session file"));
        };
        let version: u32 = rest
            .trim()
            .parse()
            .map_err(|_| format!("unreadable version in {header:?}"))?;
        if version > VERSION {
            return Err(format!(
                "this file was written by a newer CanLab (format {version}, this build reads {VERSION})"
            ));
        }

        let mut current: Option<Vec<String>> = None;
        for line in lines {
            if let Some(latex) = line.strip_prefix("line ") {
                match current.as_mut() {
                    Some(system) => system.push(latex.to_string()),
                    None => return Err(String::from("a line before the first system")),
                }
                continue;
            }
            if line.trim() == "line" {
                match current.as_mut() {
                    Some(system) => system.push(String::new()),
                    None => return Err(String::from("a line before the first system")),
                }
                continue;
            }
            if line.trim() == "system" {
                if let Some(system) = current.take() {
                    session.document.push(system);
                }
                current = Some(Vec::new());
                continue;
            }
            let mut parts = line.split_whitespace();
            match parts.next() {
                Some("palette") => {
                    // The rest of the line is the name, which may contain spaces.
                    let name = line
                        .strip_prefix("palette")
                        .map(str::trim)
                        .filter(|name| !name.is_empty());
                    match name {
                        Some(name) => session.palette = name.to_string(),
                        None => return Err(String::from("a palette with no name")),
                    }
                }
                Some("theme") => session.dark = parts.next() != Some("light"),
                Some("accent") => {
                    let channel = |part: Option<&str>| part.and_then(|p| p.parse::<u8>().ok());
                    let (r, g, b) = (channel(parts.next()), channel(parts.next()), channel(parts.next()));
                    match (r, g, b) {
                        (Some(r), Some(g), Some(b)) => session.accent = [r, g, b],
                        _ => return Err(String::from("unreadable accent colour")),
                    }
                }
                Some("range") => {
                    let low = parts.next().and_then(|p| p.parse::<f64>().ok());
                    let high = parts.next().and_then(|p| p.parse::<f64>().ok());
                    match (low, high) {
                        (Some(low), Some(high)) if high > low => session.x_range = (low, high),
                        (Some(_), Some(_)) => return Err(String::from("an empty x range")),
                        _ => return Err(String::from("unreadable plot range")),
                    }
                }
                Some("axes") => {
                    let axis = |part: Option<&str>| match part {
                        Some("-") | None => None,
                        Some(name) => Some(name.to_string()),
                    };
                    session.axes = (axis(parts.next()), axis(parts.next()));
                }
                Some("parameter") => {
                    let name = parts.next().ok_or("a parameter with no name")?;
                    let value = parts
                        .next()
                        .and_then(|p| p.parse::<f64>().ok())
                        .ok_or("a parameter with no value")?;
                    session.parameters.push((name.to_string(), value));
                }
                // Unknown directives are ignored on purpose: a field this build
                // does not use yet must not stop it reading the document.
                _ => {}
            }
        }
        if let Some(system) = current.take() {
            session.document.push(system);
        }
        if session.document.is_empty() {
            session.document.push(vec![String::new()]);
        }
        // An empty system cannot be represented by the field, so it becomes one
        // empty line rather than disappearing.
        for system in &mut session.document {
            if system.is_empty() {
                system.push(String::new());
            }
        }
        Ok(session)
    }
}

/// A float written so it reads back exactly and never looks like an integer.
fn number(value: f64) -> String {
    if !value.is_finite() {
        return String::from("0");
    }
    let mut text = format!("{value}");
    if !text.contains('.') {
        text.push_str(".0");
    }
    text
}

/// Where the session lives.
///
/// `CANLAB_SESSION` overrides for tests and for a checkout that wants its own
/// file; otherwise the platform's config directory, falling back to the working
/// directory. The lookup is a pure function of the three variables so it can be
/// tested without touching the process environment.
pub fn path() -> PathBuf {
    path_for(
        std::env::var("CANLAB_SESSION").ok(),
        std::env::var("APPDATA").ok(),
        std::env::var("HOME").ok(),
    )
}

/// [`path`], from the values it depends on.
pub fn path_for(session: Option<String>, appdata: Option<String>, home: Option<String>) -> PathBuf {
    if let Some(explicit) = session
        && !explicit.trim().is_empty()
    {
        return PathBuf::from(explicit);
    }
    if let Some(appdata) = appdata {
        return Path::new(&appdata).join("canlab").join("session.canlab");
    }
    if let Some(home) = home {
        return Path::new(&home)
            .join(".config")
            .join("canlab")
            .join("session.canlab");
    }
    PathBuf::from("session.canlab")
}

/// Save `session`, creating its directory. Returns the path written.
pub fn save(session: &Session) -> Result<PathBuf, String> {
    let path = path();
    if let Some(directory) = path.parent()
        && !directory.as_os_str().is_empty()
    {
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    }
    std::fs::write(&path, session.to_text())
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(path)
}

/// Load the session, or `None` when there is nothing (or nothing readable) there.
pub fn load() -> Option<Session> {
    let path = path();
    let text = std::fs::read_to_string(&path).ok()?;
    Session::from_text(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Session {
        Session {
            document: vec![
                vec![String::from("\\frac{x^2+1}{x}"), String::new()],
                vec![String::from("x+y=3")],
            ],
            palette: String::from("Nord"),
            dark: false,
            accent: [1, 2, 3],
            x_range: (-4.0, 4.0),
            axes: (Some(String::from("x")), Some(String::from("j"))),
            parameters: vec![(String::from("a"), 2.5), (String::from("b"), -1.0)],
        }
    }

    #[test]
    fn a_session_round_trips() {
        let session = sample();
        let text = session.to_text();
        let read = Session::from_text(&text).expect("readable");
        assert_eq!(read, session);
    }

    #[test]
    fn the_system_structure_survives() {
        // Two systems, the first of which has two lines. Flattening them on load
        // would turn two questions into one, which is the bug this guards.
        let read = Session::from_text(&sample().to_text()).expect("readable");
        assert_eq!(read.document.len(), 2);
        assert_eq!(read.document[0].len(), 2);
        assert_eq!(read.document[1].len(), 1);
    }

    #[test]
    fn an_empty_line_is_a_line() {
        let read = Session::from_text(&sample().to_text()).expect("readable");
        assert_eq!(read.document[0][1], "");
    }

    #[test]
    fn a_file_from_the_future_is_refused() {
        let error = Session::from_text("canlab 99\ntheme dark\n").unwrap_err();
        assert!(error.contains("newer"), "{error}");
    }

    #[test]
    fn something_that_is_not_a_session_is_refused() {
        assert!(Session::from_text("hello\nthere\n").is_err());
        assert!(Session::from_text("").is_err());
        assert!(Session::from_text("canlab\ntheme dark\n").is_err());
    }

    #[test]
    fn a_missing_field_takes_its_default() {
        let read = Session::from_text("canlab 1\nline x\n").unwrap_err();
        assert!(read.contains("before the first system"), "{read}");

        let read = Session::from_text("canlab 1\nsystem\nline x\n").expect("readable");
        assert_eq!(read.document, vec![vec![String::from("x")]]);
        assert!(read.dark, "the default theme is dark");
        assert_eq!(read.x_range, (-10.0, 10.0));
        assert_eq!(read.axes, (None, None));
    }

    #[test]
    fn garbage_directives_are_ignored_but_garbage_values_are_not() {
        let read = Session::from_text("canlab 1\nsystem\nline x\nfuture thing 1 2 3\n")
            .expect("an unknown directive is not fatal");
        assert_eq!(read.document[0][0], "x");

        assert!(Session::from_text("canlab 1\naccent 1 2\n").is_err());
        assert!(Session::from_text("canlab 1\nrange 3 1\n").is_err());
    }

    #[test]
    fn the_range_keeps_its_decimals() {
        let mut session = sample();
        session.x_range = (-0.5, 2.25);
        let read = Session::from_text(&session.to_text()).expect("readable");
        assert_eq!(read.x_range, (-0.5, 2.25));
    }

    #[test]
    fn a_palette_name_with_spaces_survives() {
        let read = Session::from_text("canlab 1\npalette Solarized Dark\nsystem\nline x\n")
            .expect("readable");
        assert_eq!(read.palette, "Solarized Dark");
    }

    #[test]
    fn a_session_without_a_palette_line_leaves_it_unset() {
        // Version 1 files have `theme` and `accent`; the app maps those onto a
        // scheme. If this defaulted to a name, that mapping would never happen.
        let read = Session::from_text("canlab 1\ntheme light\naccent 1 2 3\nsystem\nline x\n")
            .expect("readable");
        assert!(read.palette.is_empty());
        assert!(!read.dark);
        assert_eq!(read.accent, [1, 2, 3]);
    }

    #[test]
    fn a_path_can_be_forced() {
        // The lookup is pure, so the test does not have to mutate the process
        // environment — which parallel tests would race with.
        assert_eq!(
            path_for(Some(String::from("target/x.canlab")), None, None),
            PathBuf::from("target/x.canlab")
        );
        assert_eq!(
            path_for(Some(String::from("  ")), Some(String::from(r"C:\cfg")), None),
            Path::new(r"C:\cfg").join("canlab").join("session.canlab")
        );
        assert_eq!(
            path_for(None, None, Some(String::from("/home/u"))),
            Path::new("/home/u").join(".config").join("canlab").join("session.canlab")
        );
        assert_eq!(path_for(None, None, None), PathBuf::from("session.canlab"));
    }
}
