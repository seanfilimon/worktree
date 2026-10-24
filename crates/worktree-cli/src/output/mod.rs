//! The output layer: every command produces exactly one result, rendered
//! either as human-readable text or (with `--json`) as one JSON document on
//! stdout.

pub mod format;

use std::fmt::Display;
use std::io::{ErrorKind, Write};

use serde::Serialize;

use crate::error::{CliError, Result};

#[derive(Debug, Clone, Copy)]
pub struct Output {
    json: bool,
}

impl Output {
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    pub fn is_json(&self) -> bool {
        self.json
    }

    /// Render `value`: as JSON in `--json` mode, otherwise via `human`.
    pub fn emit<T: Serialize + ?Sized>(
        &self,
        value: &T,
        human: impl FnOnce(&mut Text),
    ) -> Result<()> {
        if self.json {
            let mut doc = serde_json::to_string_pretty(value)
                .map_err(|e| CliError::failure(format!("cannot encode JSON output: {e}")))?;
            doc.push('\n');
            write_stdout(&doc)
        } else {
            let mut text = Text::default();
            human(&mut text);
            write_stdout(text.as_str())
        }
    }
}

/// Accumulates human-readable output so it is written in one go.
#[derive(Debug, Default)]
pub struct Text(String);

impl Text {
    pub fn line(&mut self, line: impl Display) {
        self.0.push_str(&line.to_string());
        self.0.push('\n');
    }

    pub fn blank(&mut self) {
        self.0.push('\n');
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Write to stdout, treating a closed pipe (`wt log | head`) as success.
fn write_stdout(text: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(e) if e.kind() != ErrorKind::BrokenPipe => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_collects_lines() {
        let mut text = Text::default();
        text.line("one");
        text.blank();
        text.line(format_args!("{}-{}", 2, "two"));
        assert_eq!(text.as_str(), "one\n\n2-two\n");
    }

    #[test]
    fn json_flag_is_reported() {
        assert!(Output::new(true).is_json());
        assert!(!Output::new(false).is_json());
    }
}
