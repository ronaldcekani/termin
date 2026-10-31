//! Diagnostic reporting for configuration parsing and validation.

use std::fmt;

/// Severity level for configuration diagnostics.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticLevel {
    Warning,
    Error,
}

/// A diagnostic message emitted during configuration parsing or validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    pub key: Option<String>,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

impl Diagnostic {
    /// Create a warning diagnostic.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            message: message.into(),
            key: None,
            line: None,
            col: None,
        }
    }

    /// Create an error diagnostic.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            message: message.into(),
            key: None,
            line: None,
            col: None,
        }
    }

    /// Attach a configuration key name to this diagnostic.
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Attach line and column information to this diagnostic.
    pub fn with_location(mut self, line: usize, col: usize) -> Self {
        self.line = Some(line);
        self.col = Some(col);
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level_str = match self.level {
            DiagnosticLevel::Warning => "warning",
            DiagnosticLevel::Error => "error",
        };
        match (self.line, self.col, &self.key) {
            (Some(l), Some(c), Some(k)) => {
                write!(
                    f,
                    "{}:{}: {} for key '{}': {}",
                    l, c, level_str, k, self.message
                )
            }
            (Some(l), Some(c), None) => {
                write!(f, "{}:{}: {}: {}", l, c, level_str, self.message)
            }
            (None, None, Some(k)) => {
                write!(f, "{}: key '{}': {}", level_str, k, self.message)
            }
            _ => write!(f, "{}: {}", level_str, self.message),
        }
    }
}

impl std::error::Error for Diagnostic {}
