//! Tree-walking evaluator: runs statements in OpenSCAD's order (definitions,
//! then assignments, then instantiations) and builds the geometry tree.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use geometry::csg::{Node, NodeKind, SourceRef};
use rustc_hash::FxHashMap;
use scad_syntax::ast::*;
use scad_syntax::{LineIndex, SyntaxError, SyntaxWarning, SyntaxWarningKind, UnterminatedKind};

use crate::builtins;
use crate::message::{EvalError, EvalResult, Level, Message};
use crate::scope::Scope;
use crate::value::{Arguments, Closure, RangeValue, Value};

/// Nested user-function and module calls allowed before reporting
/// infinite recursion (OpenSCAD's own limit depends on its stack; 10000
/// non-tail calls fail there too). Tail calls do not count.
const MAX_CALL_DEPTH: usize = 10_000;
/// Consecutive tail calls allowed, as in OpenSCAD.
const MAX_TAIL_CALLS: usize = 1_000_000;
/// Maximum nesting of `include`.
const MAX_INCLUDE_DEPTH: usize = 64;
/// Stack kept free before growing onto a new heap segment.
const STACK_RED_ZONE: usize = 128 * 1024;
const STACK_SEGMENT: usize = 4 * 1024 * 1024;

type FileStamp = (Option<SystemTime>, u64);

struct CachedSource {
    stamp: Option<FileStamp>,
    text: Rc<str>,
    index: LineIndex,
    warnings: Vec<SyntaxWarning>,
    errors: Vec<SyntaxError>,
    tree: Rc<SourceFile>,
}

/// Reusable in-memory cache of parsed `.scad` files across multiple [`Evaluator`] runs.
///
/// Entries are keyed by canonical path and automatically invalidated whenever a file's
/// modification timestamp or byte length changes on disk.
#[derive(Clone, Default)]
pub struct SourceCache {
    entries: Rc<RefCell<FxHashMap<PathBuf, CachedSource>>>,
}

impl std::fmt::Debug for SourceCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceCache")
            .field("len", &self.entries.borrow().len())
            .finish()
    }
}

impl SourceCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    pub fn clear(&self) {
        self.entries.borrow_mut().clear();
    }
}

/// Returns standard OpenSCAD library search directories from `OPENSCADPATH` and
/// the user's platform `OpenSCAD/libraries` folder (when it exists on disk).
pub fn default_library_paths() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(env_paths) = std::env::var_os("OPENSCADPATH") {
        for p in std::env::split_paths(&env_paths) {
            if !p.as_os_str().is_empty() && !dirs.contains(&p) {
                dirs.push(p);
            }
        }
    }
    for candidate in platform_user_library_dirs() {
        if candidate.is_dir() && !dirs.contains(&candidate) {
            dirs.push(candidate);
        }
    }
    dirs
}

fn platform_user_library_dirs() -> Vec<PathBuf> {
    // No user folders in a browser (wasm32): nothing gets pushed.
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        for var in ["OneDrive", "USERPROFILE"] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                out.push(base.join("Documents").join("OpenSCAD").join("libraries"));
                out.push(base.join("Documenti").join("OpenSCAD").join("libraries"));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            out.push(
                PathBuf::from(home)
                    .join("Documents")
                    .join("OpenSCAD")
                    .join("libraries"),
            );
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            out.push(PathBuf::from(xdg).join("OpenSCAD").join("libraries"));
        }
        if let Some(home) = std::env::var_os("HOME") {
            out.push(
                PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("OpenSCAD")
                    .join("libraries"),
            );
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct EvalOptions {
    /// Directories searched by `include` / `use` after the including file's
    /// own directory (like `OPENSCADPATH` and `-L`).
    pub library_paths: Vec<PathBuf>,
    /// Top-level variable overrides (`-D name=expr` or Customizer values).
    pub defines: Vec<(String, String)>,
    /// Optional cross-run parsed file cache for fast live re-evaluation.
    pub cache: Option<SourceCache>,
    /// Value of `$preview`.
    pub preview: bool,
    /// `--enable=object-function`: enables `object()` and `has_key()`.
    pub object_function: bool,
    /// `--enable=vector-swizzle`: enables `.xyzw` / `.rgba` swizzling on vectors.
    pub vector_swizzle: bool,
    /// `--enable=import-function`: enables `import()` as an expression function.
    pub import_function: bool,
}

impl EvalOptions {
    /// Creates options pre-populated with [`default_library_paths`].
    pub fn with_standard_libraries() -> Self {
        Self {
            library_paths: default_library_paths(),
            ..Self::default()
        }
    }
}

pub struct SourceInfo {
    pub path: PathBuf,
    /// The path as OpenSCAD writes it in messages (see `display_path`).
    pub display: String,
    pub text: Rc<str>,
    pub index: LineIndex,
}

/// Human-readable source location resolved from a [`SourceRef`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSource<'a> {
    pub path: &'a Path,
    pub start_line_col: (u32, u32),
    pub end_line_col: (u32, u32),
    pub snippet: &'a str,
}

/// Children passed to the user module currently executing, for `children()`.
struct Frame<'a> {
    children: &'a [Statement],
    scope: &'a Rc<Scope>,
    file: u32,
    outer: Option<&'a Frame<'a>>,
}

enum Callee {
    User(Rc<FunctionDef>, Rc<Scope>, u32),
    Closure(Rc<Closure>),
}

impl Clone for Callee {
    fn clone(&self) -> Self {
        match self {
            Self::User(def, scope, file) => Self::User(def.clone(), scope.clone(), *file),
            Self::Closure(c) => Self::Closure(c.clone()),
        }
    }
}

enum Tail {
    Value(Value),
    /// A call to perform, with its call site and the name to report.
    Call(Callee, Arguments, Span, Name),
}

/// A call-stack entry, reported in TRACE lines when an error occurs.
enum TraceFrame {
    /// `called by 'name'` at the call site.
    CalledBy { name: Name, file: u32, span: Span },
    /// `call of 'm(a = 1, ...)'` at the module definition.
    CallOf {
        name: Name,
        args: Vec<(Name, Value)>,
        file: u32,
        span: Span,
    },
    /// `assignment to "x"` while evaluating an assignment.
    Assignment { name: Name, file: u32, span: Span },
}

/// TRACE output keeps this many frames at each end of a long stack.
const TRACE_EDGE: usize = 12;

pub struct Evaluator {
    pub(crate) files: Vec<SourceInfo>,
    /// Absolute folder of the main file, which message paths are relative to.
    main_dir: Option<PathBuf>,
    pub(crate) messages: Vec<Message>,
    /// Every active scope, innermost last: `$` variables are looked up here.
    stack: Vec<Rc<Scope>>,
    /// File the code being evaluated belongs to.
    pub(crate) file: u32,
    depth: usize,
    include_depth: usize,
    parsed: FxHashMap<PathBuf, (Rc<SourceFile>, u32)>,
    libraries: FxHashMap<PathBuf, Rc<Scope>>,
    pub(crate) module_names: Vec<Name>,
    pub(crate) rng: builtins::functions::Mt19937,
    root_modifiers: usize,
    trace: Vec<TraceFrame>,
    /// The `call of 'm()'` frame of the module being entered: OpenSCAD
    /// reports it only once the body's assignments are evaluated, so an
    /// error in `x = f();` shows `assignment to "x"` instead.
    pending_call_of: Option<TraceFrame>,
    top_scope: Option<Rc<Scope>>,
    top_nodes: Vec<Node>,
    current_root_call: Option<SourceRef>,
    /// Parsed `-D` overrides with the id of their virtual source file.
    defines: Rc<[(Name, Expr, u32)]>,
    pub(crate) options: EvalOptions,
}

impl Evaluator {
    pub fn new(options: EvalOptions) -> Self {
        Self {
            files: Vec::new(),
            main_dir: None,
            messages: Vec::new(),
            stack: Vec::new(),
            file: 0,
            depth: 0,
            include_depth: 0,
            parsed: FxHashMap::default(),
            libraries: FxHashMap::default(),
            module_names: Vec::new(),
            rng: builtins::functions::Mt19937::new(5489),
            root_modifiers: 0,
            trace: Vec::new(),
            pending_call_of: None,
            top_scope: None,
            top_nodes: Vec::new(),
            current_root_call: None,
            defines: Rc::from([]),
            options,
        }
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn take_messages(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.messages)
    }

    pub fn files(&self) -> &[SourceInfo] {
        &self.files
    }

    /// Resolves a [`SourceRef`] from a CSG [`Node`] back to file path, 1-based
    /// `(line, col)` coordinates, and the exact source snippet.
    pub fn resolve_source(&self, r: SourceRef) -> Option<ResolvedSource<'_>> {
        let info = self.files.get(r.file as usize)?;
        let start = (r.start as usize).min(info.text.len());
        let end = (r.end as usize).min(info.text.len()).max(start);
        let snippet = info.text.get(start..end).unwrap_or("");
        Some(ResolvedSource {
            path: &info.path,
            start_line_col: info.index.line_col((start as u32).into()),
            end_line_col: info.index.line_col((end as u32).into()),
            snippet,
        })
    }

    /// Evaluates a whole program and returns its top-level nodes. When an
    /// error stops evaluation, the nodes produced before it are still
    /// returned (OpenSCAD exports them too). Messages are collected in
    /// [`Self::messages`].
    pub fn run(&mut self, path: &Path, text: &str) -> (Vec<Node>, Option<EvalError>) {
        self.main_dir = absolute(path).and_then(|p| p.parent().map(Path::to_path_buf));
        let (file, id) = match self.register_parsed(path, text) {
            Ok(parsed) => parsed,
            Err(err) => return (Vec::new(), Some(err)),
        };
        self.defines = self.parse_defines();
        let root = self.root_scope();
        self.stack = vec![root.clone()];
        self.top_scope = Some(root.clone());
        self.file = id;
        let result = self.eval_block(&file.statements, &root, None);
        let nodes = std::mem::take(&mut self.top_nodes);
        (nodes, result.err())
    }

    /// Parses the `-D name=expr` overrides once. Each one becomes a small
    /// virtual source file, so messages raised while evaluating it point to
    /// the define rather than to an unrelated line of the program.
    fn parse_defines(&mut self) -> Rc<[(Name, Expr, u32)]> {
        let mut parsed = Vec::new();
        for (name, expr) in self.options.defines.clone() {
            let text = format!("{name} = {expr};");
            let tree = scad_syntax::parse(&text);
            let assignment = match tree.tree().statements.as_slice() {
                [
                    Statement {
                        kind: StatementKind::Assignment(a),
                        ..
                    },
                ] if tree.errors().is_empty() => Some(a.clone()),
                _ => None,
            };
            let Some(assignment) = assignment else {
                self.message(
                    Level::Warning,
                    format!("Ignoring invalid definition -D {name}={expr}"),
                    None,
                );
                continue;
            };
            let id = self.files.len() as u32;
            self.files.push(SourceInfo {
                path: PathBuf::from(format!("-D {name}")),
                display: format!("-D {name}"),
                index: LineIndex::new(&text),
                text: text.into(),
            });
            parsed.push((assignment.name, assignment.value, id));
        }
        parsed.into()
    }

    fn root_scope(&self) -> Rc<Scope> {
        let root = Scope::root();
        let defaults: [(&str, Value); 10] = [
            ("PI", Value::Number(std::f64::consts::PI)),
            ("$fn", Value::Number(0.0)),
            ("$fa", Value::Number(12.0)),
            ("$fs", Value::Number(2.0)),
            ("$t", Value::Number(0.0)),
            ("$preview", Value::Bool(self.options.preview)),
            (
                "$vpr",
                Value::list(vec![55.0.into(), 0.0.into(), 25.0.into()]),
            ),
            (
                "$vpt",
                Value::list(vec![0.0.into(), 0.0.into(), 0.0.into()]),
            ),
            ("$vpd", Value::Number(140.0)),
            ("$vpf", Value::Number(22.5)),
        ];
        for (name, value) in defaults {
            root.set_var(name.into(), value);
        }
        root
    }

    // ---------------------------------------------------------- files

    fn canonical_key(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn file_stamp(path: &Path) -> Option<FileStamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some((meta.modified().ok(), meta.len()))
    }

    fn register_parsed(&mut self, path: &Path, text: &str) -> EvalResult<(Rc<SourceFile>, u32)> {
        let key = Self::canonical_key(path);
        let rc_text: Rc<str> = text.into();
        let index = LineIndex::new(&rc_text);
        let parse = scad_syntax::parse(&rc_text);
        let warnings = parse.warnings().to_vec();
        let errors = parse.errors().to_vec();
        let tree = Rc::new(parse.tree());
        self.register_cached_parts(path, key, rc_text, index, &warnings, &errors, tree)
    }

    #[allow(clippy::too_many_arguments)]
    fn register_cached_parts(
        &mut self,
        display_path: &Path,
        key: PathBuf,
        text: Rc<str>,
        index: LineIndex,
        warnings: &[SyntaxWarning],
        errors: &[SyntaxError],
        tree: Rc<SourceFile>,
    ) -> EvalResult<(Rc<SourceFile>, u32)> {
        let id = self.files.len() as u32;
        self.files.push(SourceInfo {
            path: display_path.to_path_buf(),
            display: self::display_path(display_path, self.main_dir.as_deref()),
            text: text.clone(),
            index,
        });
        for warning in warnings {
            let line = self
                .files
                .get(id as usize)
                .map(|f| f.index.line(warning.range.start()));
            let (level, msg) = match &warning.kind {
                SyntaxWarningKind::DigitIdentifier(ident) => (
                    Level::Deprecated,
                    format!(
                        "Variable names starting with digits (\"{ident}\") will be removed in future releases."
                    ),
                ),
                SyntaxWarningKind::UndefinedEscape => {
                    (Level::Warning, "Undefined escape sequence".to_string())
                }
                SyntaxWarningKind::ImpreciseInteger(raw) => (
                    Level::Warning,
                    format!("Integer \"{raw}\" cannot be represented precisely"),
                ),
            };
            self.messages.push(Message {
                level,
                text: msg,
                continuation: None,
                file: Some(id),
                line,
            });
        }
        if let Some(error) = errors.first() {
            let mut err = EvalError::new("Parser error: syntax error", id, Some(error.range));
            err.parse_detail = Some(error.message.clone());
            if let Some(unterm) = error.unterminated {
                let newline_count = text.bytes().filter(|&b| b == b'\n').count() as u32;
                err.line_override = Some(newline_count + 3);
                err.pre_error = Some(match unterm {
                    UnterminatedKind::String => "Parser error: Unterminated string".into(),
                    UnterminatedKind::Comment => "Parser error: Unterminated comment".into(),
                });
            }
            return Err(err);
        }
        self.reassignment_warnings(&tree.statements, id);
        self.parsed.insert(key, (tree.clone(), id));
        Ok((tree, id))
    }

    /// Resolves an include/use path relative to the current file, then to
    /// the library directories.
    pub(crate) fn resolve_path(&self, relative: &str) -> Option<PathBuf> {
        let base = self
            .files
            .get(self.file as usize)
            .and_then(|f| f.path.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        std::iter::once(base)
            .chain(self.options.library_paths.iter().cloned())
            .map(|dir| dir.join(relative))
            .find(|candidate| candidate.is_file())
    }

    fn load_file(&mut self, relative: &str, span: Span) -> EvalResult<(Rc<SourceFile>, u32)> {
        let Some(path) = self.resolve_path(relative) else {
            return Err(self.error(format!("Can't open include file '{relative}'."), span));
        };
        let key = Self::canonical_key(&path);
        if let Some(found) = self.parsed.get(&key) {
            return Ok(found.clone());
        }
        if let Some(cache) = self.options.cache.clone() {
            let current_stamp = Self::file_stamp(&key);
            let hit = cache.entries.borrow().get(&key).and_then(|entry| {
                (current_stamp.is_some() && entry.stamp == current_stamp).then(|| {
                    (
                        entry.text.clone(),
                        entry.index.clone(),
                        entry.warnings.clone(),
                        entry.errors.clone(),
                        entry.tree.clone(),
                    )
                })
            });
            if let Some((text, index, warnings, errors, tree)) = hit {
                return self
                    .register_cached_parts(&path, key, text, index, &warnings, &errors, tree);
            }
            let raw_text = std::fs::read_to_string(&path).map_err(|e| {
                self.error(format!("Can't read file '{}': {e}", path.display()), span)
            })?;
            let text: Rc<str> = raw_text.into();
            let index = LineIndex::new(&text);
            let parse = scad_syntax::parse(&text);
            let warnings = parse.warnings().to_vec();
            let errors = parse.errors().to_vec();
            let tree = Rc::new(parse.tree());
            cache.entries.borrow_mut().insert(
                key.clone(),
                CachedSource {
                    stamp: current_stamp,
                    text: text.clone(),
                    index: index.clone(),
                    warnings: warnings.clone(),
                    errors: errors.clone(),
                    tree: tree.clone(),
                },
            );
            return self.register_cached_parts(&path, key, text, index, &warnings, &errors, tree);
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| self.error(format!("Can't read file '{}': {e}", path.display()), span))?;
        self.register_parsed(&path, &text)
    }

    /// Inlines `include`d statements, remembering each one's file.
    fn expand_includes(
        &mut self,
        statements: &[Statement],
        file: u32,
        out: &mut Vec<(Statement, u32)>,
    ) -> EvalResult<()> {
        for statement in statements {
            if let StatementKind::Include(path) = &statement.kind {
                if self.include_depth >= MAX_INCLUDE_DEPTH {
                    return Err(self.error("Include nesting too deep", statement.span));
                }
                let saved = self.file;
                self.file = file;
                if self.resolve_path(path).is_none() {
                    self.warning(format!("Can't find include file '{path}'."), statement.span);
                    self.file = saved;
                    continue;
                }
                let (included, id) = match self.load_file(path, statement.span) {
                    Ok(loaded) => loaded,
                    Err(err) => {
                        self.file = saved;
                        return Err(err);
                    }
                };
                self.include_depth += 1;
                let result = self.expand_includes(&included.statements, id, out);
                self.include_depth -= 1;
                self.file = saved;
                result?;
            } else {
                out.push((statement.clone(), file));
            }
        }
        Ok(())
    }

    /// `use <file>`: makes the file's functions and modules visible.
    fn use_library(&mut self, relative: &str, scope: &Rc<Scope>, span: Span) -> EvalResult<()> {
        let Some(path) = self.resolve_path(relative) else {
            self.warning(format!("Can't open library '{relative}'."), span);
            return Ok(());
        };
        let key = Self::canonical_key(&path);
        if let Some(library) = self.libraries.get(&key) {
            scope.add_used(library.clone());
            return Ok(());
        }
        // A library that fails to parse is reported and skipped.
        let (tree, id) = match self.load_file(relative, span) {
            Ok(loaded) => loaded,
            Err(err) if err.is_parse_error() => {
                self.report_error(&err);
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        // A library has its own global scope: it cannot see the caller's
        // definitions or variables.
        let library = self.root_scope();
        // Remember the library before evaluating it so cyclic `use` stops.
        self.libraries.insert(key, library.clone());

        let saved_file = self.file;
        let saved_stack = std::mem::replace(&mut self.stack, vec![library.clone()]);
        let saved_top = self.top_scope.take();
        self.file = id;
        // Only definitions and assignments run: an empty selection skips
        // every top-level instantiation (echo, geometry…).
        let result = self.eval_block_selected(&tree.statements, &library, None, Some(&[]));
        self.stack = saved_stack;
        self.top_scope = saved_top;
        self.file = saved_file;
        result?;
        scope.add_used(library);
        Ok(())
    }

    // ----------------------------------------------------- diagnostics

    pub(crate) fn message(&mut self, level: Level, text: impl Into<String>, span: Option<Span>) {
        self.message_with_continuation(level, text, None, span);
    }

    pub(crate) fn message_with_continuation(
        &mut self,
        level: Level,
        text: impl Into<String>,
        continuation: Option<&'static str>,
        span: Option<Span>,
    ) {
        let file = self.file;
        let line = span.and_then(|s| {
            self.files
                .get(file as usize)
                .map(|f| f.index.line(s.start()))
        });
        self.messages.push(Message {
            level,
            text: text.into(),
            continuation,
            file: span.map(|_| file),
            line,
        });
    }

    /// Records an error as a message (for errors that do not stop evaluation).
    pub fn report_error(&mut self, err: &EvalError) {
        let line = err.line_override.or_else(|| {
            err.span.and_then(|s| {
                self.files
                    .get(err.file as usize)
                    .map(|f| f.index.line(s.start()))
            })
        });
        let file = err
            .span
            .map(|_| err.file)
            .or_else(|| err.line_override.map(|_| err.file));
        if let Some(pre) = &err.pre_error {
            self.messages.push(Message {
                level: Level::Error,
                text: pre.clone(),
                continuation: None,
                file,
                line,
            });
        }
        self.messages.push(Message {
            level: Level::Error,
            text: err.message.clone(),
            continuation: None,
            file,
            line,
        });
    }

    pub(crate) fn warning(&mut self, text: impl Into<String>, span: Span) {
        self.message(Level::Warning, text, Some(span));
    }

    pub(crate) fn error(&self, text: impl Into<String>, span: Span) -> EvalError {
        let mut error = EvalError::new(text, self.file, Some(span));
        error.trace = self.format_trace();
        error
    }

    /// ` in file f.scad, line N`
    pub fn location(&self, file: u32, span: Span) -> String {
        self.files.get(file as usize).map_or_else(String::new, |f| {
            format!(
                " in file {}, line {}",
                f.display,
                f.index.line(span.start())
            )
        })
    }

    fn format_trace(&self) -> Vec<String> {
        let format = |frame: &TraceFrame| match frame {
            TraceFrame::CalledBy { name, file, span } => {
                format!("called by '{name}'{}", self.location(*file, *span))
            }
            TraceFrame::CallOf {
                name,
                args,
                file,
                span,
            } => {
                let args: Vec<String> = args
                    .iter()
                    .map(|(n, v)| format!("{n} = {}", v.to_repr()))
                    .collect();
                format!(
                    "call of '{name}({})'{}",
                    args.join(", "),
                    self.location(*file, *span)
                )
            }
            TraceFrame::Assignment { name, file, span } => {
                format!("assignment to \"{name}\"{}", self.location(*file, *span))
            }
        };
        let frames: Vec<&TraceFrame> = self.trace.iter().rev().collect();
        if frames.len() <= 2 * TRACE_EDGE {
            return frames.into_iter().map(format).collect();
        }
        let excluded = frames.len() - 2 * TRACE_EDGE;
        let mut lines: Vec<String> = frames[..TRACE_EDGE].iter().map(|f| format(f)).collect();
        lines.push(format!("  *** Excluding {excluded} frames ***"));
        lines.extend(
            frames[frames.len() - TRACE_EDGE..]
                .iter()
                .map(|f| format(f)),
        );
        lines
    }

    pub(crate) fn source_ref(&self, span: Span) -> SourceRef {
        SourceRef {
            file: self.file,
            start: span.start().into(),
            end: span.end().into(),
        }
    }

    // ------------------------------------------------- scopes and stack

    /// Runs `f` with `scope` pushed on the dynamic stack.
    fn with_scope<T>(&mut self, scope: &Rc<Scope>, f: impl FnOnce(&mut Self) -> T) -> T {
        let base = self.stack.len();
        self.stack.push(scope.clone());
        let result = f(self);
        self.stack.truncate(base);
        result
    }

    /// Value of a `$` variable, searched through the dynamic stack.
    pub(crate) fn special(&self, name: &str) -> Value {
        self.stack
            .iter()
            .rev()
            .find_map(|scope| scope.local_var(name))
            .unwrap_or_default()
    }

    /// A variable's value, `None` when it does not exist. `$` variables
    /// are searched through the dynamic stack, except `$children`: like
    /// OpenSCAD, it is the number of children of the module whose body
    /// contains the code, also when that code runs as children of another
    /// module (BOSL2's `attachable() { if ($children > 0) … }`).
    fn find_variable(&self, name: &str, scope: &Rc<Scope>) -> Option<Value> {
        if name.starts_with('$') && name != "$children" {
            self.stack
                .iter()
                .rev()
                .find_map(|s| s.local_var(name))
                .or_else(|| scope.lookup_var(name))
        } else {
            scope.lookup_var(name)
        }
    }

    fn lookup_variable(&mut self, name: &str, scope: &Rc<Scope>, span: Span) -> Value {
        self.find_variable(name, scope).unwrap_or_else(|| {
            self.warning(format!("Ignoring unknown variable \"{name}\""), span);
            Value::Undef
        })
    }

    fn enter_call(&mut self, what: &str, span: Span) -> EvalResult<()> {
        if self.depth >= MAX_CALL_DEPTH {
            return Err(self.error(format!("Recursion detected calling {what}"), span));
        }
        self.depth += 1;
        Ok(())
    }

    // ------------------------------------------------------- statements

    fn eval_block(
        &mut self,
        statements: &[Statement],
        scope: &Rc<Scope>,
        frame: Option<&Frame<'_>>,
    ) -> EvalResult<Vec<Node>> {
        self.eval_block_selected(statements, scope, frame, None)
    }

    /// Evaluates a block; `select` restricts which instantiations (by
    /// index among the block's instantiations) produce geometry.
    fn eval_block_selected(
        &mut self,
        statements: &[Statement],
        scope: &Rc<Scope>,
        frame: Option<&Frame<'_>>,
        select: Option<&[usize]>,
    ) -> EvalResult<Vec<Node>> {
        let file = self.file;
        let expanded;
        let items: Vec<(&Statement, u32)> = if statements
            .iter()
            .any(|s| matches!(s.kind, StatementKind::Include(_)))
        {
            let mut owned = Vec::new();
            self.expand_includes(statements, file, &mut owned)?;
            expanded = owned;
            expanded.iter().map(|(s, f)| (s, *f)).collect()
        } else {
            statements.iter().map(|s| (s, file)).collect()
        };
        let result = self.eval_items(&items, scope, frame, select);
        self.file = file;
        result
    }

    fn eval_items(
        &mut self,
        items: &[(&Statement, u32)],
        scope: &Rc<Scope>,
        frame: Option<&Frame<'_>>,
        select: Option<&[usize]>,
    ) -> EvalResult<Vec<Node>> {
        let call_of = self.pending_call_of.take();
        // 1. Definitions are visible in the whole block.
        for &(statement, file) in items {
            match &statement.kind {
                StatementKind::ModuleDef(def) => scope.define_module(def.clone(), file),
                StatementKind::FunctionDef(def) => scope.define_function(def.clone(), file),
                StatementKind::Use(path) => {
                    self.file = file;
                    self.use_library(path, scope, statement.span)?;
                }
                _ => {}
            }
        }

        let is_top = self
            .top_scope
            .as_ref()
            .is_some_and(|top| Rc::ptr_eq(top, scope));

        // 2. Assignments, in order. A reassigned variable takes its last
        //    value at the position of its first assignment (the warning is
        //    emitted when the file is parsed, see `reassignment_warnings`).
        // `-D` overrides apply to the main program's top-level scope only.
        let defines = if is_top {
            self.defines.clone()
        } else {
            Rc::from([])
        };

        let mut order: Vec<(&Name, &Expr, u32, Option<Span>)> = Vec::new();
        let mut position: FxHashMap<&Name, usize> = FxHashMap::default();
        for &(statement, file) in items {
            if let StatementKind::Assignment(assignment) = &statement.kind {
                match position.get(&assignment.name) {
                    Some(&i) => {
                        order[i].1 = &assignment.value;
                        order[i].2 = file;
                        order[i].3 = Some(statement.span);
                    }
                    None => {
                        position.insert(&assignment.name, order.len());
                        order.push((
                            &assignment.name,
                            &assignment.value,
                            file,
                            Some(statement.span),
                        ));
                    }
                }
            }
        }
        // Like an assignment appended to the file: it replaces the expression
        // of an existing variable at its original position.
        for (def_name, def_expr, def_file) in defines.iter() {
            match position.get(def_name) {
                Some(&i) => order[i] = (def_name, def_expr, *def_file, None),
                None => {
                    position.insert(def_name, order.len());
                    order.push((def_name, def_expr, *def_file, None));
                }
            }
        }
        for (name, expr, file, span) in order {
            self.file = file;
            if let Some(span) = span {
                self.trace.push(TraceFrame::Assignment {
                    name: name.clone(),
                    file,
                    span,
                });
            }
            let value = self.eval_expr(expr, scope)?;
            if span.is_some() {
                self.trace.pop();
            }
            scope.set_var(name.clone(), value);
        }
        if let Some(call_of) = call_of {
            self.trace.push(call_of);
        }

        // 3. Instantiations.
        let mut nodes = Vec::new();
        let mut index = 0;
        for &(statement, file) in items {
            if let StatementKind::Instantiation(inst) = &statement.kind {
                let selected = select.is_none_or(|s| s.contains(&index));
                index += 1;
                if !selected {
                    continue;
                }
                self.file = file;
                let prev_root = if is_top {
                    self.current_root_call
                        .replace(self.source_ref(statement.span))
                } else {
                    self.current_root_call
                };
                let inst_res = self.eval_instantiation(inst, statement.span, scope, frame);
                if is_top {
                    self.current_root_call = prev_root;
                }
                if let Some(node) = inst_res? {
                    // Top-level nodes are kept outside so they survive errors.
                    if is_top {
                        self.top_nodes.push(node);
                    } else {
                        nodes.push(node);
                    }
                }
            }
        }
        Ok(nodes)
    }

    /// Parse-time warnings for variables assigned twice in the same scope,
    /// for every block of a file (including modules never instantiated).
    fn reassignment_warnings(&mut self, statements: &[Statement], file: u32) {
        let saved = self.file;
        self.file = file;
        let mut first_line: FxHashMap<&Name, u32> = FxHashMap::default();
        for statement in statements {
            if let StatementKind::Assignment(a) = &statement.kind {
                let line = self.files[file as usize].index.line(statement.span.start());
                match first_line.get(&a.name) {
                    Some(&first) => self.warning(
                        format!(
                            "\"{}\" was assigned on line {first} but was overwritten",
                            a.name
                        ),
                        statement.span,
                    ),
                    None => {
                        first_line.insert(&a.name, line);
                    }
                }
            }
        }
        for statement in statements {
            match &statement.kind {
                StatementKind::ModuleDef(def) => self.reassignment_warnings(&def.body, file),
                StatementKind::Instantiation(inst) => match &inst.kind {
                    InstantiationKind::Call { children, .. } => {
                        self.reassignment_warnings(children, file);
                    }
                    InstantiationKind::If {
                        then_branch,
                        else_branch,
                        ..
                    } => {
                        self.reassignment_warnings(then_branch, file);
                        if let Some(else_branch) = else_branch {
                            self.reassignment_warnings(else_branch, file);
                        }
                    }
                },
                _ => {}
            }
        }
        self.file = saved;
    }

    fn eval_instantiation(
        &mut self,
        inst: &Instantiation,
        span: Span,
        scope: &Rc<Scope>,
        frame: Option<&Frame<'_>>,
    ) -> EvalResult<Option<Node>> {
        if inst.modifiers.disable {
            return Ok(None);
        }
        if inst.modifiers.root {
            self.root_modifiers += 1;
            if self.root_modifiers > 1 {
                self.warning("More than one Root Modifier (!)", span);
            }
        }
        let node = stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || match &inst.kind {
            InstantiationKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                // `called by 'if'`, at the condition (where OpenSCAD puts it).
                let trace_base = self.trace.len();
                self.trace.push(TraceFrame::CalledBy {
                    name: "if".into(),
                    file: self.file,
                    span: condition.span,
                });
                let result = (|| {
                    let branch = if self.eval_expr(condition, scope)?.is_truthy() {
                        Some(then_branch)
                    } else {
                        else_branch.as_ref()
                    };
                    // A false `if` without `else` produces no node at all.
                    let Some(statements) = branch else {
                        return Ok(None);
                    };
                    let inner = Scope::child(scope);
                    let children =
                        self.with_scope(&inner, |ev| ev.eval_block(statements, &inner, frame))?;
                    Ok(Some(Node::group("if", children)))
                })();
                self.trace.truncate(trace_base);
                result
            }
            InstantiationKind::Call {
                name,
                name_span,
                args,
                children,
            } => {
                let trace_base = self.trace.len();
                self.trace.push(TraceFrame::CalledBy {
                    name: name.clone(),
                    file: self.file,
                    span: *name_span,
                });
                let result =
                    self.eval_module_call(name, *name_span, args, children, span, scope, frame);
                self.trace.truncate(trace_base);
                result
            }
        })?;

        Ok(node.map(|mut node| {
            node.modifiers.root = inst.modifiers.root;
            node.modifiers.highlight = inst.modifiers.highlight;
            node.modifiers.background = inst.modifiers.background;
            node.with_source(self.source_ref(span))
                .with_root_call(self.current_root_call)
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_module_call(
        &mut self,
        name: &Name,
        name_span: Span,
        args: &[Argument],
        children: &[Statement],
        span: Span,
        scope: &Rc<Scope>,
        frame: Option<&Frame<'_>>,
    ) -> EvalResult<Option<Node>> {
        match name.as_str() {
            "children" => return self.eval_children(args, scope, frame, span),
            "for" | "intersection_for" => {
                return self
                    .eval_for_module(name, args, children, scope, frame)
                    .map(Some);
            }
            "let" => {
                let inner = Scope::child(scope);
                let nodes = self.with_scope(&inner, |ev| {
                    ev.bind_sequential(args, &inner)?;
                    ev.eval_block(children, &inner, frame)
                })?;
                // Without children these statements leave no node.
                return Ok((!children.is_empty()).then(|| Node::group("let", nodes)));
            }
            "echo" | "assert" => {
                let values = self.eval_args(args, scope)?;
                if name == "echo" {
                    self.echo(&values, Some(span));
                } else {
                    self.check_assert(args, &values, span)?;
                }
                let inner = Scope::child(scope);
                let nodes = self.with_scope(&inner, |ev| ev.eval_block(children, &inner, frame))?;
                return Ok((!children.is_empty()).then(|| Node::group(name.as_str(), nodes)));
            }
            _ => {}
        }

        if let Some(((def, def_file), def_scope)) = scope.lookup_module(name) {
            let values = self.eval_args(args, scope)?;
            return self
                .call_user_module(
                    &def, def_file, &def_scope, values, children, scope, frame, span,
                )
                .map(Some);
        }

        // Builtin module: `$` arguments apply to the module and its children.
        let values = self.eval_args(args, scope)?;
        let call_scope = Scope::child(scope);
        for (arg_name, value) in &values.named {
            if arg_name.starts_with('$') {
                call_scope.set_var(arg_name.clone(), value.clone());
            }
        }
        self.with_scope(&call_scope, |ev| {
            if !builtins::modules::exists(name) {
                ev.warning(format!("Ignoring unknown module '{name}'"), name_span);
                return Ok(None);
            }
            // OpenSCAD checks a builtin's parameters before instantiating its
            // children: its warnings go before theirs.
            let before_children = ev.messages.len();
            let child_nodes = if !builtins::modules::accepts_children(name) {
                if !children.is_empty() {
                    ev.warning(
                        format!("module {name}() does not support child modules"),
                        span,
                    );
                }
                Vec::new()
            } else {
                let inner = Scope::child(&call_scope);
                ev.with_scope(&inner, |ev| ev.eval_block(children, &inner, frame))?
            };
            let after_children = ev.messages.len();
            let node = builtins::modules::instantiate(ev, name, &values, child_nodes, span);
            if after_children > before_children && ev.messages.len() > after_children {
                let own: Vec<Message> = ev.messages.drain(after_children..).collect();
                ev.messages.splice(before_children..before_children, own);
            }
            node
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn call_user_module(
        &mut self,
        def: &Rc<ModuleDef>,
        def_file: u32,
        def_scope: &Rc<Scope>,
        args: Arguments,
        children: &[Statement],
        caller_scope: &Rc<Scope>,
        frame: Option<&Frame<'_>>,
        span: Span,
    ) -> EvalResult<Node> {
        self.enter_call(&format!("module '{}'", def.name), span)?;
        let caller_file = self.file;
        let call_scope = Scope::child(def_scope);
        let base = self.stack.len();
        self.stack.push(call_scope.clone());
        self.module_names.push(def.name.clone());

        let trace_base = self.trace.len();
        let result = (|| {
            self.file = def_file;
            self.bind_params(&def.params, args, &call_scope, def_scope, span)?;
            self.pending_call_of = Some(TraceFrame::CallOf {
                name: def.name.clone(),
                args: def
                    .params
                    .iter()
                    .map(|p| {
                        (
                            p.name.clone(),
                            call_scope.local_var(&p.name).unwrap_or_default(),
                        )
                    })
                    .collect(),
                file: def_file,
                span: def.span,
            });
            let count = children
                .iter()
                .filter(|s| matches!(s.kind, StatementKind::Instantiation(_)))
                .count();
            call_scope.set_var("$children".into(), Value::Number(count as f64));
            call_scope.set_var(
                "$parent_modules".into(),
                Value::Number(self.module_names.len() as f64),
            );
            let inner = Frame {
                children,
                scope: caller_scope,
                file: caller_file,
                outer: frame,
            };
            self.eval_block(&def.body, &call_scope, Some(&inner))
        })();

        self.pending_call_of = None;
        self.module_names.pop();
        self.stack.truncate(base);
        self.trace.truncate(trace_base);
        self.file = caller_file;
        self.depth -= 1;
        Ok(Node::group(def.name.as_str(), result?))
    }

    fn eval_children(
        &mut self,
        args: &[Argument],
        scope: &Rc<Scope>,
        frame: Option<&Frame<'_>>,
        span: Span,
    ) -> EvalResult<Option<Node>> {
        let values = self.eval_args(args, scope)?;
        let Some(frame) = frame else {
            return Ok(None);
        };
        let count = frame
            .children
            .iter()
            .filter(|s| matches!(s.kind, StatementKind::Instantiation(_)))
            .count();
        let selection: Option<Vec<i64>> = match values.positional.first() {
            None => None,
            Some(Value::Number(n)) => Some(vec![*n as i64]),
            Some(Value::List(items)) => Some(
                items
                    .iter()
                    .filter_map(Value::as_number)
                    .map(|n| n as i64)
                    .collect(),
            ),
            Some(Value::Range(r)) => Some(r.iter().map(|n| n as i64).collect()),
            Some(other) => {
                self.warning(
                    format!(
                        "Bad parameter type ({}) for children, only accept: empty, number, vector, range",
                        other.to_repr()
                    ),
                    span,
                );
                return Ok(None);
            }
        };
        let in_range = |i: i64| (0..count as i64).contains(&i);
        let selection: Option<Vec<usize>> = match selection {
            None => None,
            Some(selection) => {
                for &bad in selection.iter().filter(|&&i| !in_range(i)) {
                    self.warning(
                        format!("Children index ({bad}) out of bounds ({count} children)"),
                        span,
                    );
                }
                let valid: Vec<usize> = selection
                    .into_iter()
                    .filter(|&i| in_range(i))
                    .map(|i| i as usize)
                    .collect();
                if valid.is_empty() {
                    return Ok(None);
                }
                Some(valid)
            }
        };

        let saved_file = self.file;
        let inner = Scope::child(frame.scope);
        self.file = frame.file;
        let result = self.with_scope(&inner, |ev| {
            ev.eval_block_selected(frame.children, &inner, frame.outer, selection.as_deref())
        });
        self.file = saved_file;
        Ok(Some(Node::group("children", result?)))
    }

    fn eval_for_module(
        &mut self,
        name: &str,
        args: &[Argument],
        children: &[Statement],
        scope: &Rc<Scope>,
        frame: Option<&Frame<'_>>,
    ) -> EvalResult<Node> {
        let mut iterations = Vec::new();
        self.for_module_level(args, children, scope, frame, &mut iterations)?;
        let nodes = iterations.into_iter().flatten().collect();
        if name == "intersection_for" {
            Ok(Node::new(NodeKind::Intersection, nodes))
        } else {
            Ok(Node::group("for", nodes))
        }
    }

    /// Nested loops: the first assignment is the outermost.
    fn for_module_level(
        &mut self,
        args: &[Argument],
        children: &[Statement],
        scope: &Rc<Scope>,
        frame: Option<&Frame<'_>>,
        out: &mut Vec<Vec<Node>>,
    ) -> EvalResult<()> {
        let Some((first, rest)) = args.split_first() else {
            let inner = Scope::child(scope);
            out.push(self.with_scope(&inner, |ev| ev.eval_block(children, &inner, frame))?);
            return Ok(());
        };
        let values = self.eval_expr(&first.value, scope)?;
        for value in self.iteration_values(&values, first.value.span)? {
            let inner = Scope::child(scope);
            if let Some(var) = &first.name {
                inner.set_var(var.clone(), value);
            }
            self.with_scope(&inner, |ev| {
                ev.for_module_level(rest, children, &inner, frame, out)
            })?;
        }
        Ok(())
    }

    /// Values a `for` iterates over.
    fn iteration_values(&mut self, value: &Value, span: Span) -> EvalResult<Vec<Value>> {
        Ok(match value {
            Value::List(items) => items.to_vec(),
            Value::Range(range) => {
                if !self.range_iterable(range, span) {
                    return Ok(Vec::new());
                }
                range.iter().map(Value::Number).collect()
            }
            Value::String(s) => s.chars().map(|c| Value::string(c.to_string())).collect(),
            Value::Object(entries) => entries
                .iter()
                .map(|(k, _)| Value::string(k.as_str()))
                .collect(),
            Value::Undef => Vec::new(),
            other => vec![other.clone()],
        })
    }

    /// Ranges with a million elements or more are refused with a warning.
    fn range_iterable(&mut self, range: &RangeValue, span: Span) -> bool {
        let len = range.len();
        if len >= crate::value::MAX_RANGE_ELEMENTS {
            self.warning(
                format!("Bad range parameter in for statement: too many elements ({len})"),
                span,
            );
            return false;
        }
        true
    }

    pub(crate) fn echo(&mut self, args: &Arguments, span: Option<Span>) {
        let mut positional = args.positional.iter();
        let mut named = args.named.iter();
        let parts: Vec<String> = args
            .named_order
            .iter()
            .filter_map(|&is_named| {
                if is_named {
                    named
                        .next()
                        .map(|(name, value)| format!("{name} = {}", value.to_repr()))
                } else {
                    positional.next().map(Value::to_repr)
                }
            })
            .collect();
        self.message(Level::Echo, parts.join(", "), span);
    }

    fn check_assert(
        &mut self,
        args: &[Argument],
        values: &Arguments,
        span: Span,
    ) -> EvalResult<()> {
        let condition = values.positional.first().or_else(|| {
            values
                .named
                .iter()
                .find(|(n, _)| n == "condition")
                .map(|(_, v)| v)
        });
        if condition.is_some_and(Value::is_truthy) {
            return Ok(());
        }
        let mut message = match args.first() {
            Some(arg) => format!(
                "Assertion '{}' failed",
                crate::print::expr_to_string(&arg.value)
            ),
            None => "Assertion failed".to_string(),
        };
        if let Some(extra) = values.positional.get(1).or_else(|| {
            values
                .named
                .iter()
                .find(|(n, _)| n == "message")
                .map(|(_, v)| v)
        }) {
            message.push_str(": ");
            message.push_str(&extra.to_repr());
        }
        Err(self.error(message, span))
    }

    // ------------------------------------------------------- arguments

    pub(crate) fn eval_args(
        &mut self,
        args: &[Argument],
        scope: &Rc<Scope>,
    ) -> EvalResult<Arguments> {
        let mut out = Arguments::default();
        for arg in args {
            let value = self.eval_expr(&arg.value, scope)?;
            out.named_order.push(arg.name.is_some());
            match &arg.name {
                Some(name) => out.named.push((name.clone(), value)),
                None => out.positional.push(value),
            }
        }
        Ok(out)
    }

    /// `let`-style sequential bindings: each sees the previous ones. A
    /// repeated name keeps its first value.
    fn bind_sequential(&mut self, args: &[Argument], scope: &Rc<Scope>) -> EvalResult<()> {
        for arg in args {
            let Some(name) = &arg.name else {
                self.eval_expr(&arg.value, scope)?;
                continue;
            };
            if scope.has_local_var(name) {
                self.warning(
                    format!(
                        "Ignoring duplicate variable assignment \"{name}\" = {}",
                        crate::print::expr_to_string(&arg.value)
                    ),
                    arg.span,
                );
                continue;
            }
            let value = self.eval_expr(&arg.value, scope)?;
            scope.set_var(name.clone(), value);
        }
        Ok(())
    }

    /// Binds call arguments to parameters in `scope`, then evaluates the
    /// defaults of the parameters left unset.
    /// Binds call arguments to parameters in `scope`. Defaults of unset
    /// parameters are evaluated in the definition scope (`def_scope`), so
    /// they cannot see the other parameters.
    fn bind_params(
        &mut self,
        params: &[Param],
        args: Arguments,
        scope: &Rc<Scope>,
        def_scope: &Rc<Scope>,
        span: Span,
    ) -> EvalResult<()> {
        let mut set = vec![false; params.len()];
        if args.positional.len() > params.len() {
            self.warning("Too many unnamed arguments supplied", span);
        }
        // Positional arguments fill the parameters not given by name.
        let named: Vec<&Name> = args.named.iter().map(|(n, _)| n).collect();
        let mut free = params
            .iter()
            .enumerate()
            .filter(|(_, p)| !named.contains(&&p.name))
            .map(|(i, _)| i);
        for value in args.positional {
            if let Some(i) = free.next() {
                scope.set_var(params[i].name.clone(), value);
                set[i] = true;
            }
        }
        let mut seen: Vec<Name> = Vec::new();
        for (name, value) in args.named {
            if seen.contains(&name) {
                self.warning(format!("argument \"{name}\" supplied more than once"), span);
            }
            seen.push(name.clone());
            match params.iter().position(|p| p.name == name) {
                Some(i) => {
                    scope.set_var(name, value);
                    set[i] = true;
                }
                None if name.starts_with('$') => scope.set_var(name, value),
                None => {
                    self.warning(
                        format!("variable \"{name}\" not specified as parameter"),
                        span,
                    );
                }
            }
        }
        for (param, was_set) in params.iter().zip(set) {
            if !was_set {
                let value = match &param.default {
                    Some(default) => self.eval_expr(default, def_scope)?,
                    None => Value::Undef,
                };
                scope.set_var(param.name.clone(), value);
            }
        }
        Ok(())
    }

    // ------------------------------------------------------ expressions

    pub(crate) fn eval_expr(&mut self, expr: &Expr, scope: &Rc<Scope>) -> EvalResult<Value> {
        stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || {
            self.eval_expr_inner(expr, scope)
        })
    }

    fn eval_expr_inner(&mut self, expr: &Expr, scope: &Rc<Scope>) -> EvalResult<Value> {
        Ok(match &expr.kind {
            ExprKind::Undef => Value::Undef,
            ExprKind::Bool(b) => Value::Bool(*b),
            ExprKind::Number(n) => Value::Number(*n),
            ExprKind::String(s) => Value::String(s.clone()),
            ExprKind::Variable(name) => self.lookup_variable(name, scope, expr.span),
            ExprKind::Unary { op, operand } => {
                let value = self.eval_expr(operand, scope)?;
                let (result, symbol) = match op {
                    UnaryOp::Neg => (value.neg(), "-"),
                    UnaryOp::Plus => match value {
                        Value::Number(_) | Value::List(_) => (value.clone(), "+"),
                        _ => (Value::Undef, "+"),
                    },
                    UnaryOp::Not => (Value::Bool(!value.is_truthy()), "!"),
                    UnaryOp::BitNot => (value.bit_not(), "~"),
                };
                if result.is_undef() {
                    self.warning(
                        format!("undefined operation ({symbol}{})", value.type_name()),
                        expr.span,
                    );
                }
                result
            }
            ExprKind::Binary { op, lhs, rhs } => {
                self.eval_binary(*op, lhs, rhs, scope, expr.span)?
            }
            ExprKind::Ternary { .. }
            | ExprKind::Let { .. }
            | ExprKind::Assert { .. }
            | ExprKind::Echo { .. }
            | ExprKind::Call { .. } => match self.eval_tail(expr, scope)? {
                Tail::Value(value) => value,
                Tail::Call(callee, args, span, name) => {
                    self.call_function(callee, args, span, name)?
                }
            },
            ExprKind::Index { target, index } => {
                let target = self.eval_expr(target, scope)?;
                let index = self.eval_expr(index, scope)?;
                target.index(&index)
            }
            ExprKind::Member { target, member } => self
                .eval_expr(target, scope)?
                .member_with_swizzle(member, self.options.vector_swizzle),
            ExprKind::Range {
                start: start_expr,
                step: step_expr,
                end: end_expr,
            } => {
                let start = self.eval_expr(start_expr, scope)?;
                let step = match step_expr {
                    Some(step) => self.eval_expr(step, scope)?,
                    None => Value::Number(1.0),
                };
                let end = self.eval_expr(end_expr, scope)?;
                // OpenSCAD checks the direction only of ranges written with
                // number literals (`[0:1:-1]`), not computed ones
                // (`[0:1:n-1]`, BOSL2's usual empty loop).
                let literal = is_number_literal(start_expr)
                    && step_expr.as_deref().is_none_or(is_number_literal)
                    && is_number_literal(end_expr);
                match (start, step, end) {
                    (start, _, end)
                        if !matches!(start, Value::Number(_))
                            || !matches!(end, Value::Number(_)) =>
                    {
                        self.warning(
                            format!(
                                "Unable to convert [{}:...:{}] to a range",
                                start.to_repr(),
                                end.to_repr()
                            ),
                            expr.span,
                        );
                        Value::Undef
                    }
                    (Value::Number(start), Value::Number(step), Value::Number(end)) => {
                        if literal && step > 0.0 && start > end {
                            self.warning(
                                "begin is greater than the end, but step is positive",
                                expr.span,
                            );
                        } else if literal && step < 0.0 && start < end {
                            self.warning(
                                "begin is smaller than the end, but step is negative",
                                expr.span,
                            );
                        }
                        Value::Range(RangeValue { start, step, end })
                    }
                    (_, step, _) => {
                        self.warning(
                            format!(
                                "Unable to convert [...:{}:...] to a step value",
                                step.to_repr()
                            ),
                            expr.span,
                        );
                        Value::Undef
                    }
                }
            }
            ExprKind::List(elements) => {
                let mut out = Vec::with_capacity(elements.len());
                for element in elements {
                    self.eval_list_element(element, scope, &mut out)?;
                }
                Value::list(out)
            }
            ExprKind::FunctionLiteral(literal) => Value::Function(Rc::new(Closure {
                literal: literal.clone(),
                scope: scope.clone(),
                file: self.file,
            })),
        })
    }

    fn eval_binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        scope: &Rc<Scope>,
        span: Span,
    ) -> EvalResult<Value> {
        // Logical operators short-circuit.
        match op {
            BinaryOp::And => {
                let left = self.eval_expr(lhs, scope)?.is_truthy();
                return Ok(Value::Bool(left && self.eval_expr(rhs, scope)?.is_truthy()));
            }
            BinaryOp::Or => {
                let left = self.eval_expr(lhs, scope)?.is_truthy();
                return Ok(Value::Bool(left || self.eval_expr(rhs, scope)?.is_truthy()));
            }
            _ => {}
        }
        let a = self.eval_expr(lhs, scope)?;
        let b = self.eval_expr(rhs, scope)?;
        use std::cmp::Ordering::*;
        let compare = |accept: &[std::cmp::Ordering]| match a.compare(&b) {
            Some(ordering) => Value::Bool(accept.contains(&ordering)),
            // NaN compares false; other incomparable pairs are undefined.
            None if a.as_number().is_some() && b.as_number().is_some() => Value::Bool(false),
            None => Value::Undef,
        };
        if matches!(op, BinaryOp::Mul)
            && let (Value::List(x), Value::List(y)) = (&a, &b)
            && let Some(problem) = crate::value::vector_product_problem(x, y)
        {
            self.warning(problem, span);
            return Ok(Value::Undef);
        }
        if matches!(op, BinaryOp::Shl | BinaryOp::Shr)
            && let (Value::Number(_), Value::Number(shift)) = (&a, &b)
        {
            let shift = *shift as i64;
            if !(0..64).contains(&shift) {
                let reason = if shift < 0 {
                    "negative shift"
                } else {
                    "shift too large"
                };
                self.warning(reason, span);
                return Ok(Value::Undef);
            }
        }
        let result = match op {
            BinaryOp::Add => a.add(&b),
            BinaryOp::Sub => a.sub(&b),
            BinaryOp::Mul => a.mul(&b),
            BinaryOp::Div => a.div(&b),
            BinaryOp::Mod => a.rem(&b),
            BinaryOp::Pow => a.pow(&b),
            BinaryOp::Lt => compare(&[Less]),
            BinaryOp::Le => compare(&[Less, Equal]),
            BinaryOp::Gt => compare(&[Greater]),
            BinaryOp::Ge => compare(&[Greater, Equal]),
            BinaryOp::Eq => Value::Bool(a.equals(&b)),
            BinaryOp::Ne => Value::Bool(!a.equals(&b)),
            BinaryOp::BitAnd => a.bitwise(&b, |x, y| x & y),
            BinaryOp::BitOr => a.bitwise(&b, |x, y| x | y),
            BinaryOp::Shl => a.bitwise(&b, |x, y| x << y),
            BinaryOp::Shr => a.bitwise(&b, |x, y| x >> y),
            BinaryOp::And | BinaryOp::Or => unreachable!("handled above"),
        };
        if result.is_undef() {
            self.warning(
                format!(
                    "undefined operation ({} {} {})",
                    a.type_name(),
                    crate::print::binary_symbol(op),
                    b.type_name()
                ),
                span,
            );
        }
        Ok(result)
    }

    /// Evaluates an expression in tail position: calls to user functions
    /// are returned instead of performed, so recursion runs in a loop.
    fn eval_tail(&mut self, expr: &Expr, scope: &Rc<Scope>) -> EvalResult<Tail> {
        let mut expr = expr;
        let mut scope = scope.clone();
        loop {
            match &expr.kind {
                ExprKind::Ternary {
                    condition,
                    then_expr,
                    else_expr,
                } => {
                    expr = if self.eval_expr(condition, &scope)?.is_truthy() {
                        then_expr
                    } else {
                        else_expr
                    };
                }
                ExprKind::Let { assignments, body } => {
                    let inner = Scope::child(&scope);
                    self.stack.push(inner.clone());
                    self.bind_sequential(assignments, &inner)?;
                    scope = inner;
                    expr = body;
                }
                ExprKind::Assert { args, body } => {
                    let values = self.eval_args(args, &scope)?;
                    self.check_assert(args, &values, expr.span)?;
                    match body {
                        Some(body) => expr = body,
                        None => return Ok(Tail::Value(Value::Undef)),
                    }
                }
                ExprKind::Echo { args, body } => {
                    let values = self.eval_args(args, &scope)?;
                    self.echo(&values, Some(expr.span));
                    match body {
                        Some(body) => expr = body,
                        None => return Ok(Tail::Value(Value::Undef)),
                    }
                }
                ExprKind::Call { callee, args } => {
                    return self.eval_call(callee, args, &scope, expr.span);
                }
                _ => return Ok(Tail::Value(self.eval_expr(expr, &scope)?)),
            }
        }
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Argument],
        scope: &Rc<Scope>,
        span: Span,
    ) -> EvalResult<Tail> {
        let display_name: Name = match &callee.kind {
            ExprKind::Variable(name) => name.clone(),
            _ => "function".into(),
        };
        let target = if let ExprKind::Variable(name) = &callee.kind {
            if let Some(((def, file), def_scope)) = scope.lookup_function(name) {
                Some(Callee::User(def, def_scope, file))
            } else if let Some(Value::Function(closure)) = scope.lookup_var(name) {
                Some(Callee::Closure(closure))
            } else if builtins::functions::is_disabled_experimental(name, &self.options) {
                self.warning(
                    format!("Experimental builtin function '{name}' is not enabled"),
                    callee.span,
                );
                self.warning(format!("Ignoring unknown function '{name}'"), callee.span);
                return Ok(Tail::Value(Value::Undef));
            } else if builtins::functions::exists(name, &self.options) {
                // `is_undef(x)` asks whether `x` exists: OpenSCAD gives no
                // "unknown variable" warning for it.
                if &**name == "is_undef"
                    && let [
                        Argument {
                            name: None,
                            value:
                                Expr {
                                    kind: ExprKind::Variable(var),
                                    ..
                                },
                            ..
                        },
                    ] = args
                {
                    let value = self.find_variable(var, scope).unwrap_or_default();
                    return Ok(Tail::Value(Value::Bool(value.is_undef())));
                }
                let values = self.eval_args(args, scope)?;
                return builtins::functions::call(self, name, values, span).map(Tail::Value);
            } else {
                self.warning(format!("Ignoring unknown function '{name}'"), callee.span);
                return Ok(Tail::Value(Value::Undef));
            }
        } else {
            match self.eval_expr(callee, scope)? {
                Value::Function(closure) => Some(Callee::Closure(closure)),
                other => {
                    self.warning(
                        format!("Can't call a value of type {}", other.type_name()),
                        callee.span,
                    );
                    None
                }
            }
        };
        match target {
            Some(callee) => Ok(Tail::Call(
                callee,
                self.eval_args(args, scope)?,
                span,
                display_name,
            )),
            None => Ok(Tail::Value(Value::Undef)),
        }
    }

    /// Calls a user function or closure, looping over tail calls. A tail
    /// call replaces the caller's trace frame instead of adding one.
    fn call_function(
        &mut self,
        callee: Callee,
        args: Arguments,
        span: Span,
        name: Name,
    ) -> EvalResult<Value> {
        self.enter_call(&format!("function '{name}'"), span)?;
        let base = self.stack.len();
        let trace_base = self.trace.len();
        let saved_file = self.file;
        self.trace.push(TraceFrame::CalledBy {
            name: name.clone(),
            file: self.file,
            span,
        });

        let mut callee = callee;
        let mut args = args;
        let mut name = name;
        let mut call_span = span;
        let mut tail_calls = 0;
        // 1 once a tail call left the `$` variables of its caller on the stack.
        let mut base_extra = 0;
        let result = loop {
            tail_calls += 1;
            if tail_calls > MAX_TAIL_CALLS {
                break Err(self.error(
                    format!("Recursion detected calling function '{name}'"),
                    call_span,
                ));
            }
            self.stack.truncate(base + base_extra);
            let current = callee.clone();
            let (params, body, def_scope, file) = match &current {
                Callee::User(def, scope, file) => (&def.params, &def.body, scope, *file),
                Callee::Closure(c) => (&c.literal.params, &c.literal.body, &c.scope, c.file),
            };
            let scope = Scope::child(def_scope);
            self.stack.push(scope.clone());
            self.file = file;
            let step = self
                .bind_params(params, args, &scope, def_scope, span)
                .and_then(|()| self.eval_tail(body, &scope));
            match step {
                Ok(Tail::Value(value)) => break Ok(value),
                Ok(Tail::Call(next, next_args, next_span, next_name)) => {
                    // The next call replaces this one on the stack, but must
                    // still see its `$` variables (`f(n, $fn = 24)` calling
                    // itself in tail position): they are merged into one
                    // scope, so deep recursion does not grow the stack.
                    let carried = Scope::child(def_scope);
                    for frame in &self.stack[base..] {
                        frame.copy_special_vars_to(&carried);
                    }
                    self.stack.truncate(base);
                    self.stack.push(carried);
                    base_extra = 1;
                    self.trace.truncate(trace_base);
                    self.trace.push(TraceFrame::CalledBy {
                        name: next_name.clone(),
                        file: self.file,
                        span: next_span,
                    });
                    callee = next;
                    args = next_args;
                    name = next_name;
                    call_span = next_span;
                }
                Err(error) => break Err(error),
            }
        };

        self.stack.truncate(base);
        self.trace.truncate(trace_base);
        self.file = saved_file;
        self.depth -= 1;
        result
    }

    // ------------------------------------------------ list comprehension

    fn eval_list_element(
        &mut self,
        element: &ListElement,
        scope: &Rc<Scope>,
        out: &mut Vec<Value>,
    ) -> EvalResult<()> {
        match &element.kind {
            ListElementKind::Expr(expr) => out.push(self.eval_expr(expr, scope)?),
            ListElementKind::Each(inner) => {
                let mut produced = Vec::new();
                self.eval_list_element(inner, scope, &mut produced)?;
                for value in produced {
                    match value {
                        Value::Undef => {}
                        Value::List(items) => out.extend(items.iter().cloned()),
                        Value::Range(range) => out.extend(range.iter().map(Value::Number)),
                        Value::String(s) => {
                            out.extend(s.chars().map(|c| Value::string(c.to_string())));
                        }
                        other => out.push(other),
                    }
                }
            }
            ListElementKind::For { assignments, body } => {
                self.list_for_level(assignments, body, scope, out)?;
            }
            ListElementKind::ForC {
                init,
                condition,
                update,
                body,
            } => {
                let mut current = Scope::child(scope);
                let base = self.stack.len();
                self.stack.push(current.clone());
                let result = (|| {
                    self.bind_sequential(init, &current)?;
                    while self.eval_expr(condition, &current)?.is_truthy() {
                        self.eval_list_element(body, &current, out)?;
                        // Updates run in order, each seeing the ones before
                        // it (`v = p[i+1] - p[i], c = v*v, i = i+1`); a name
                        // updated twice keeps its first value, as in OpenSCAD.
                        let step = Scope::child(&current);
                        self.stack.push(step.clone());
                        for arg in update {
                            let value = self.eval_expr(&arg.value, &step)?;
                            let Some(name) = &arg.name else { continue };
                            if step.has_local_var(name) {
                                self.warning(
                                    format!(
                                        "Ignoring duplicate variable assignment \"{name}\" = {}",
                                        value.to_repr()
                                    ),
                                    arg.span,
                                );
                            } else {
                                step.set_var(name.clone(), value);
                            }
                        }
                        let next = Scope::child(scope);
                        let names = init.iter().chain(update).filter_map(|a| a.name.as_ref());
                        for name in names {
                            if let Some(value) =
                                step.local_var(name).or_else(|| current.local_var(name))
                            {
                                next.set_var(name.clone(), value);
                            }
                        }
                        self.stack.truncate(base);
                        self.stack.push(next.clone());
                        current = next;
                    }
                    Ok(())
                })();
                self.stack.truncate(base);
                result?;
            }
            ListElementKind::If {
                condition,
                then_element,
                else_element,
            } => {
                if self.eval_expr(condition, scope)?.is_truthy() {
                    self.eval_list_element(then_element, scope, out)?;
                } else if let Some(else_element) = else_element {
                    self.eval_list_element(else_element, scope, out)?;
                }
            }
            ListElementKind::Let { assignments, body } => {
                let inner = Scope::child(scope);
                self.with_scope(&inner, |ev| {
                    ev.bind_sequential(assignments, &inner)?;
                    ev.eval_list_element(body, &inner, out)
                })?;
            }
        }
        Ok(())
    }

    fn list_for_level(
        &mut self,
        assignments: &[Argument],
        body: &ListElement,
        scope: &Rc<Scope>,
        out: &mut Vec<Value>,
    ) -> EvalResult<()> {
        let Some((first, rest)) = assignments.split_first() else {
            return self.eval_list_element(body, scope, out);
        };
        let values = self.eval_expr(&first.value, scope)?;
        let iterate = |ev: &mut Self, value: Value, out: &mut Vec<Value>| {
            let inner = Scope::child(scope);
            if let Some(name) = &first.name {
                inner.set_var(name.clone(), value);
            }
            ev.with_scope(&inner, |ev| ev.list_for_level(rest, body, &inner, out))
        };
        // Ranges are walked lazily: they can be long.
        if let Value::Range(range) = &values {
            if !self.range_iterable(range, first.value.span) {
                return Ok(());
            }
            for n in range.iter() {
                iterate(self, Value::Number(n), out)?;
            }
            return Ok(());
        }
        for value in self.iteration_values(&values, first.value.span)? {
            iterate(self, value, out)?;
        }
        Ok(())
    }
}

/// `path` made absolute against the current folder, with `.` and `..`
/// resolved without touching the disk. `None` without a current folder
/// (WebAssembly).
fn absolute(path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// How OpenSCAD names a file in messages: relative to the main file's
/// folder, with `..` where needed (`../lib/x.scad`); absolute when no
/// relative path exists (another drive). Always with `/`.
pub(crate) fn display_path(path: &Path, main_dir: Option<&Path>) -> String {
    let slashes = |p: &Path| p.display().to_string().replace('\\', "/");
    let (Some(main_dir), Some(path)) = (main_dir, absolute(path)) else {
        return slashes(path);
    };
    let base: Vec<_> = main_dir.components().collect();
    let target: Vec<_> = path.components().collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return slashes(&path);
    }
    let mut parts = vec!["..".to_string(); base.len() - common];
    parts.extend(
        target[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// A number written in the code, possibly signed (`3`, `-1`, `-(-1)`).
fn is_number_literal(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Number(_) => true,
        ExprKind::Unary {
            op: UnaryOp::Neg | UnaryOp::Plus,
            operand,
        } => is_number_literal(operand),
        _ => false,
    }
}
