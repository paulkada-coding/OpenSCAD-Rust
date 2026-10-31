//! Diagnostics produced while evaluating.

use scad_syntax::ast::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Echo,
    Deprecated,
    Warning,
    Error,
    Trace,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Self::Echo => "ECHO",
            Self::Deprecated => "DEPRECATED",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
            Self::Trace => "TRACE",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub level: Level,
    pub text: String,
    /// File id (index into the evaluator's files) and 1-based line.
    pub file: Option<u32>,
    pub line: Option<u32>,
}

/// An error that stops evaluation (assert failure, recursion, parse error…).
#[derive(Debug, Clone, PartialEq)]
pub struct EvalError {
    pub message: String,
    pub file: u32,
    pub span: Option<Span>,
    /// Explicit 1-based line override (used for EOF errors like unterminated
    /// strings/comments where OpenSCAD reports a synthetic line past EOF).
    pub line_override: Option<u32>,
    /// Optional preceding `ERROR:` line emitted right before `message`.
    pub pre_error: Option<String>,
    /// Set for syntax errors; holds the parser's detailed explanation.
    pub parse_detail: Option<String>,
    /// Call stack when the error happened, innermost first, already
    /// formatted like OpenSCAD's TRACE lines (without the prefix).
    pub trace: Vec<String>,
}

impl EvalError {
    pub fn new(message: impl Into<String>, file: u32, span: Option<Span>) -> Self {
        Self {
            message: message.into(),
            file,
            span,
            line_override: None,
            pre_error: None,
            parse_detail: None,
            trace: Vec::new(),
        }
    }

    pub fn is_parse_error(&self) -> bool {
        self.parse_detail.is_some()
    }
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EvalError {}

pub type EvalResult<T> = Result<T, EvalError>;
