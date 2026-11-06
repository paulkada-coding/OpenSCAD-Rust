//! Code editor for the open `.scad` file: syntax highlighting from our own
//! lexer, line numbers, underlined parse errors, the code picked on the model
//! shown in blue, and saving that keeps the file's encoding details (byte
//! order mark, CRLF line endings).
//!
//! The buffer, not the file on disk, is what gets evaluated, so the model
//! can follow the code while it is typed.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Stroke, TextBuffer, TextFormat,
    text::{ByteIndex, CCursor, LayoutJob, LayoutSection, TextWrapping},
    text_selection::CCursorRange,
};
use scad_customizer::write_file_atomic;
use scad_syntax::{SyntaxKind, lexer::lex};

use crate::worker::modified_time;

/// How long typing must pause before the buffer is evaluated.
pub const TYPING_DELAY: Duration = Duration::from_millis(300);

/// Longest list of parse errors shown above the code.
const MAX_LISTED_ERRORS: usize = 3;

/// Background of the code picked on the model: the viewport's selection blue.
const PICKED_CODE: Color32 = Color32::from_rgba_premultiplied(26, 72, 130, 120);

/// Built-in modules and functions, colored apart from user code.
const BUILTINS: &[&str] = &[
    // 3D and 2D primitives
    "cube",
    "sphere",
    "cylinder",
    "polyhedron",
    "square",
    "circle",
    "polygon",
    "text",
    "import",
    "surface",
    // transformations and operations
    "translate",
    "rotate",
    "scale",
    "resize",
    "mirror",
    "multmatrix",
    "color",
    "offset",
    "hull",
    "minkowski",
    "union",
    "difference",
    "intersection",
    "linear_extrude",
    "rotate_extrude",
    "projection",
    "render",
    "children",
    "parent_module",
    // functions
    "abs",
    "sign",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "atan2",
    "floor",
    "round",
    "ceil",
    "ln",
    "log",
    "pow",
    "sqrt",
    "exp",
    "min",
    "max",
    "norm",
    "cross",
    "len",
    "concat",
    "lookup",
    "str",
    "chr",
    "ord",
    "search",
    "rands",
    "version",
    "version_num",
    "is_undef",
    "is_bool",
    "is_num",
    "is_string",
    "is_list",
    "is_function",
    "is_object",
    "object",
    "fontmetrics",
    "textmetrics",
];

/// How the file was stored, restored on save.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FileFormat {
    /// UTF-8 byte order mark at the start (Notepad writes one).
    bom: bool,
    /// Windows line endings. The buffer always uses `\n`.
    crlf: bool,
}

impl FileFormat {
    fn encode(self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 16);
        if self.bom {
            out.push('\u{feff}');
        }
        if self.crlf {
            out.push_str(&text.replace('\n', "\r\n"));
        } else {
            out.push_str(text);
        }
        out
    }
}

/// File contents as the editor holds them.
#[derive(Debug, PartialEq, Eq)]
struct Decoded {
    text: String,
    format: FileFormat,
    /// Why the file cannot be edited safely.
    read_only: Option<String>,
}

fn decode(bytes: Vec<u8>) -> Decoded {
    let (text, read_only) = match String::from_utf8(bytes) {
        Ok(text) => (text, None),
        Err(err) => (
            String::from_utf8_lossy(err.as_bytes()).into_owned(),
            Some("The file is not UTF-8 text: editing is disabled so it is not damaged.".into()),
        ),
    };
    let bom = text.starts_with('\u{feff}');
    let body = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let crlf = body.contains("\r\n");
    Decoded {
        text: if crlf {
            body.replace("\r\n", "\n")
        } else {
            body.to_owned()
        },
        format: FileFormat { bom, crlf },
        read_only,
    }
}

/// A parse error, located in the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Diagnostic {
    /// Never empty, so it can be underlined.
    range: Range<usize>,
    /// 1-based.
    line: usize,
    message: String,
}

fn diagnostics_of(text: &str) -> Vec<Diagnostic> {
    scad_syntax::parse(text)
        .errors()
        .iter()
        .filter_map(|err| {
            let start = usize::from(err.range.start()).min(text.len());
            let end = usize::from(err.range.end()).min(text.len());
            let range = visible_range(text, start, end)?;
            Some(Diagnostic {
                line: line_of(text, range.start),
                range,
                message: err.message.clone(),
            })
        })
        .collect()
}

/// An empty range grows to the next character (or the previous one at the
/// end of the text), so there is something to underline.
fn visible_range(text: &str, start: usize, end: usize) -> Option<Range<usize>> {
    if start < end {
        return Some(start..end);
    }
    if let Some(c) = text[start..].chars().next()
        && c != '\n'
    {
        return Some(start..start + c.len_utf8());
    }
    let c = text[..start].chars().next_back()?;
    Some(start - c.len_utf8()..start)
}

/// 1-based line of a byte offset.
fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Byte offset → character index, as egui's cursors count.
fn char_index(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text.char_indices().take_while(|&(i, _)| i < offset).count()
}

/// Character index (egui's cursors) → byte offset.
fn byte_offset(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(i, _)| i)
}

/// The largest character boundary not after `offset`.
fn floor_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

// ---------------------------------------------------------------- colors

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Role {
    Plain,
    Keyword,
    Constant,
    Number,
    Str,
    Comment,
    Builtin,
    Definition,
    Special,
    Invalid,
}

fn color(role: Role, dark: bool, plain: Color32) -> Color32 {
    let rgb = |d: u32, l: u32| {
        let c = if dark { d } else { l };
        Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
    };
    match role {
        Role::Plain => plain,
        Role::Keyword => rgb(0xC586C0, 0xAF00DB),
        Role::Constant => rgb(0x569CD6, 0x0000FF),
        Role::Number => rgb(0xB5CEA8, 0x098658),
        Role::Str => rgb(0xCE9178, 0xA31515),
        Role::Comment => rgb(0x6A9955, 0x008000),
        Role::Builtin => rgb(0x4EC9B0, 0x267F99),
        Role::Definition => rgb(0xDCDCAA, 0x795E26),
        Role::Special => rgb(0x9CDCFE, 0x001080),
        Role::Invalid => rgb(0xF44747, 0xCD3131),
    }
}

fn role_of(kind: SyntaxKind, text: &str, after_definition_keyword: bool) -> Role {
    use SyntaxKind as K;
    match kind {
        K::LINE_COMMENT | K::BLOCK_COMMENT | K::UNTERMINATED_COMMENT => Role::Comment,
        K::NUMBER | K::OVERFLOW_NUMBER => Role::Number,
        K::STRING | K::UNTERMINATED_STRING => Role::Str,
        K::INCLUDE | K::USE => Role::Keyword,
        K::MODULE_KW
        | K::FUNCTION_KW
        | K::IF_KW
        | K::ELSE_KW
        | K::FOR_KW
        | K::LET_KW
        | K::EACH_KW
        | K::ASSERT_KW
        | K::ECHO_KW => Role::Keyword,
        K::TRUE_KW | K::FALSE_KW | K::UNDEF_KW => Role::Constant,
        K::IDENT if after_definition_keyword => Role::Definition,
        K::IDENT if text.starts_with('$') => Role::Special,
        K::IDENT if BUILTINS.contains(&text) || text == "intersection_for" || text == "PI" => {
            Role::Builtin
        }
        K::ERROR_TOKEN => Role::Invalid,
        _ => Role::Plain,
    }
}

/// What the colored text depends on, to rebuild it only when needed.
#[derive(Hash)]
struct JobKey<'a> {
    text: &'a str,
    picked: &'a Option<Range<usize>>,
    dark: bool,
    plain: Color32,
    font: &'a FontId,
}

#[derive(Default)]
struct Highlighter {
    key: u64,
    job: Option<LayoutJob>,
    diagnostics: Vec<Diagnostic>,
}

impl Highlighter {
    fn update(&mut self, text: &str, picked: &Option<Range<usize>>, ui: &egui::Ui, font: &FontId) {
        let dark = ui.visuals().dark_mode;
        let plain = ui.visuals().text_color();
        let mut hasher = DefaultHasher::new();
        JobKey {
            text,
            picked,
            dark,
            plain,
            font,
        }
        .hash(&mut hasher);
        let key = hasher.finish();
        if self.job.is_some() && key == self.key {
            return;
        }
        self.key = key;
        self.diagnostics = diagnostics_of(text);
        let underline = Stroke::new(1.5, color(Role::Invalid, dark, plain));
        self.job = Some(build_job(
            text,
            &self.diagnostics,
            picked.clone(),
            font,
            |role| color(role, dark, plain),
            underline,
        ));
    }
}

fn build_job(
    text: &str,
    diagnostics: &[Diagnostic],
    picked: Option<Range<usize>>,
    font: &FontId,
    color_of: impl Fn(Role) -> Color32,
    underline: Stroke,
) -> LayoutJob {
    // Offsets may be stale by a frame while typing: keep them inside the
    // text and on character boundaries.
    let clamp = |r: &Range<usize>| floor_boundary(text, r.start)..floor_boundary(text, r.end);
    let errors: Vec<Range<usize>> = diagnostics.iter().map(|d| clamp(&d.range)).collect();
    let picked = picked.as_ref().map(clamp);
    let mut cuts: Vec<usize> = errors
        .iter()
        .chain(picked.iter())
        .flat_map(|r| [r.start, r.end])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();

    let mut job = LayoutJob {
        text: text.to_owned(),
        wrap: TextWrapping::no_max_width(),
        ..LayoutJob::default()
    };
    let mut push = |range: Range<usize>, role: Role| {
        if range.is_empty() {
            return;
        }
        let inside = |r: &Range<usize>| r.start <= range.start && range.end <= r.end;
        let format = TextFormat {
            font_id: font.clone(),
            color: color_of(role),
            background: if picked.as_ref().is_some_and(inside) {
                PICKED_CODE
            } else {
                Color32::TRANSPARENT
            },
            underline: if errors.iter().any(inside) {
                underline
            } else {
                Stroke::NONE
            },
            ..TextFormat::default()
        };
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(range.start)..ByteIndex(range.end),
            format,
        });
    };

    let mut offset = 0;
    let mut after_definition_keyword = false;
    for token in lex(text) {
        let start = offset;
        let end = start + token.text.len();
        offset = end;
        let role = role_of(token.kind, token.text, after_definition_keyword);
        if !token.kind.is_trivia() {
            after_definition_keyword =
                matches!(token.kind, SyntaxKind::MODULE_KW | SyntaxKind::FUNCTION_KW);
        }
        let mut from = start;
        for &cut in cuts.iter().filter(|&&c| start < c && c < end) {
            push(from..cut, role);
            from = cut;
        }
        push(from..end, role);
    }
    // The lexer covers every byte; this only guards against a mismatch.
    push(offset.min(text.len())..text.len(), Role::Plain);
    job
}

// ---------------------------------------------------------------- editor

/// What the user did in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorEvent {
    None,
    /// The text was edited.
    Changed,
    /// Save was clicked.
    Save,
    /// The file was reloaded from disk, discarding the edits.
    Reloaded,
}

pub struct Editor {
    path: PathBuf,
    text: String,
    /// The text as last loaded or saved.
    saved: String,
    format: FileFormat,
    /// Modification time of the file when last loaded or saved.
    disk_mtime: Option<SystemTime>,
    read_only: Option<String>,
    /// Bumped by every change of the text.
    revision: u64,
    /// When the text last changed and has not been evaluated yet.
    changed_at: Option<Instant>,
    /// The code picked on the model, as a byte range of the text.
    picked: Option<Range<usize>>,
    /// Move the cursor to this offset and scroll to it on the next frame.
    reveal: Option<usize>,
    /// The file changed on disk while the buffer had unsaved edits.
    conflict: bool,
    /// Character index of the cursor as last seen, to notice moves.
    last_cursor: Option<usize>,
    /// Byte offset the user moved the cursor to, not yet taken by the app.
    cursor_moved: Option<usize>,
    highlighter: Highlighter,
}

impl Editor {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let disk_mtime = modified_time(path);
        let decoded = decode(std::fs::read(path)?);
        Ok(Self {
            path: path.to_path_buf(),
            saved: decoded.text.clone(),
            text: decoded.text,
            format: decoded.format,
            disk_mtime,
            read_only: decoded.read_only,
            revision: 0,
            changed_at: None,
            picked: None,
            reveal: None,
            conflict: false,
            last_cursor: None,
            cursor_moved: None,
            highlighter: Highlighter::default(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text to evaluate; `None` when the file cannot be edited, so it
    /// is read from disk as it is.
    pub fn source(&self) -> Option<String> {
        self.read_only.is_none().then(|| self.text.clone())
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.text != self.saved
    }

    pub fn is_editable(&self) -> bool {
        self.read_only.is_none()
    }

    /// Writes the buffer to its file.
    pub fn save(&mut self) -> std::io::Result<()> {
        if let Some(reason) = &self.read_only {
            return Err(std::io::Error::other(reason.clone()));
        }
        write_file_atomic(&self.path, self.format.encode(&self.text).as_bytes())?;
        self.saved = self.text.clone();
        self.disk_mtime = modified_time(&self.path);
        self.conflict = false;
        Ok(())
    }

    /// Replaces the buffer with the file on disk.
    pub fn reload_from_disk(&mut self) -> std::io::Result<()> {
        let disk_mtime = modified_time(&self.path);
        let decoded = decode(std::fs::read(&self.path)?);
        self.format = decoded.format;
        self.read_only = decoded.read_only;
        self.saved = decoded.text.clone();
        self.set_text(decoded.text);
        self.disk_mtime = disk_mtime;
        self.changed_at = None;
        self.conflict = false;
        Ok(())
    }

    /// Whether the file changed on disk since it was loaded or saved.
    pub fn disk_changed(&self) -> bool {
        modified_time(&self.path) != self.disk_mtime
    }

    /// The file changed on disk but the buffer has edits: ask the user.
    pub fn set_conflict(&mut self) {
        self.conflict = true;
        self.disk_mtime = modified_time(&self.path);
    }

    /// Replaces the text (e.g. parameters applied by the Customizer); the
    /// change can be undone with Ctrl+Z like typing.
    pub fn set_text(&mut self, text: String) {
        if text != self.text {
            self.text = text;
            self.revision += 1;
            self.picked = None;
        }
    }

    /// Consumes a pending change once typing has paused for `delay`.
    pub fn take_due_change(&mut self, delay: Duration) -> bool {
        match self.changed_at {
            Some(at) if at.elapsed() >= delay => {
                self.changed_at = None;
                true
            }
            _ => false,
        }
    }

    /// Time left before a pending change is due, to wake the UI.
    pub fn pending_delay(&self, delay: Duration) -> Option<Duration> {
        self.changed_at.map(|at| delay.saturating_sub(at.elapsed()))
    }

    /// Forgets a pending change (it is being evaluated now).
    pub fn clear_pending_change(&mut self) {
        self.changed_at = None;
    }

    /// Shows the code picked on the model; with `reveal`, also moves the
    /// cursor to it and scrolls it into view.
    pub fn show_picked(&mut self, range: Option<Range<usize>>, reveal: bool) {
        let range = range.filter(|r| r.end <= self.text.len() && r.start <= r.end);
        if reveal {
            self.reveal = range.as_ref().map(|r| r.start);
        }
        self.picked = range;
    }

    /// Where the user moved the cursor (click or arrow keys, not typing)
    /// since the last call, as a byte offset of the text.
    pub fn take_cursor_move(&mut self) -> Option<usize> {
        self.cursor_moved.take()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> EditorEvent {
        let mut event = EditorEvent::None;
        let dirty = self.is_dirty();
        ui.horizontal(|ui| {
            ui.strong(file_name(&self.path))
                .on_hover_text(self.path.display().to_string());
            if dirty {
                ui.label("●").on_hover_text("Unsaved changes");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        dirty && self.read_only.is_none(),
                        egui::Button::new("Save").small(),
                    )
                    .on_hover_text("Save the file (Ctrl+S)")
                    .clicked()
                {
                    event = EditorEvent::Save;
                }
            });
        });
        if let Some(reason) = &self.read_only {
            ui.colored_label(ui.visuals().warn_fg_color, reason);
        }
        if self.conflict {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "The file changed on disk, and you have unsaved changes.",
            );
            ui.horizontal(|ui| {
                if ui
                    .small_button("Reload from disk")
                    .on_hover_text("Discard your changes and load the file")
                    .clicked()
                    && self.reload_from_disk().is_ok()
                {
                    event = EditorEvent::Reloaded;
                }
                if ui
                    .small_button("Keep mine")
                    .on_hover_text("Keep editing; saving will replace the file")
                    .clicked()
                {
                    self.conflict = false;
                }
            });
        }

        let font = egui::TextStyle::Monospace.resolve(ui.style());
        self.highlighter.update(&self.text, &self.picked, ui, &font);
        self.errors_ui(ui);
        ui.separator();

        let line_count = self.text.bytes().filter(|&b| b == b'\n').count() + 1;
        let digits = line_count.to_string().len().max(2);
        let digit_width = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        let gutter = (digit_width * digits as f32 + 14.0).round().min(120.0);
        let error_lines: Vec<usize> = self
            .highlighter
            .diagnostics
            .iter()
            .map(|d| d.line)
            .collect();

        let id = ui.make_persistent_id("code_editor");
        let editable = self.read_only.is_none();
        let reveal = self.reveal.take();
        egui::ScrollArea::both()
            .id_salt("code_editor_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let Self {
                    text,
                    highlighter,
                    picked,
                    ..
                } = self;
                let mut layouter = |ui: &egui::Ui, buffer: &dyn TextBuffer, _wrap: f32| {
                    highlighter.update(buffer.as_str(), picked, ui, &font);
                    let job = highlighter.job.clone().unwrap_or_default();
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let output = egui::TextEdit::multiline(text)
                    .id(id)
                    .code_editor()
                    .font(font.clone())
                    .interactive(editable)
                    .desired_width(f32::INFINITY)
                    .min_size(ui.available_size())
                    .margin(egui::Margin {
                        left: gutter as i8,
                        right: 4,
                        top: 2,
                        bottom: 2,
                    })
                    .frame(egui::Frame::NONE)
                    .layouter(&mut layouter)
                    .show(ui);

                let cursor = output.cursor_range.map(|r| r.primary.index.0);
                if output.response.changed() {
                    self.revision += 1;
                    self.changed_at = Some(Instant::now());
                    // The picked code's offsets no longer match the text.
                    self.picked = None;
                    event = EditorEvent::Changed;
                } else if let Some(index) = cursor
                    && cursor != self.last_cursor
                {
                    self.cursor_moved = Some(byte_offset(&self.text, index));
                }
                self.last_cursor = cursor;

                if let Some(offset) = reveal {
                    let cursor = CCursor::new(char_index(&self.text, offset));
                    // Moved by the app, not the user: not a move to report.
                    self.last_cursor = Some(cursor.index.0);
                    let mut state = output.state.clone();
                    state.cursor.set_char_range(Some(CCursorRange::one(cursor)));
                    state.store(ui.ctx(), id);
                    let rect = output
                        .galley
                        .pos_from_cursor(cursor)
                        .translate(output.galley_pos.to_vec2());
                    // Align on the left edge: horizontal scroll back to the
                    // start of the line, vertical scroll to its middle.
                    let target = egui::Rect::from_min_max(
                        egui::pos2(output.galley_pos.x - gutter, rect.top()),
                        egui::pos2(output.galley_pos.x, rect.bottom()),
                    );
                    ui.scroll_to_rect(target, Some(Align::Center));
                }

                // Line numbers, painted over the left margin so they stay in
                // view when the code scrolls sideways.
                let clip = ui.clip_rect();
                let strip = egui::Rect::from_min_max(
                    clip.left_top(),
                    egui::pos2(clip.left() + gutter - 6.0, clip.bottom()),
                );
                let painter = ui.painter();
                painter.rect_filled(strip, 0.0, ui.visuals().panel_fill);
                let weak = ui.visuals().weak_text_color();
                let error = ui.visuals().error_fg_color;
                let mut line = 1;
                let mut line_start = true;
                for row in &output.galley.rows {
                    let top = output.galley_pos.y + row.pos.y;
                    if line_start && top <= clip.bottom() && top + row.row.size.y >= clip.top() {
                        painter.text(
                            egui::pos2(strip.right() - 4.0, top),
                            Align2::RIGHT_TOP,
                            line.to_string(),
                            font.clone(),
                            if error_lines.contains(&line) {
                                error
                            } else {
                                weak
                            },
                        );
                    }
                    line_start = row.ends_with_newline;
                    if row.ends_with_newline {
                        line += 1;
                    }
                }
            });
        event
    }

    /// Parse errors, clickable to jump to them.
    fn errors_ui(&mut self, ui: &mut egui::Ui) {
        let diagnostics = &self.highlighter.diagnostics;
        if diagnostics.is_empty() {
            return;
        }
        let color = ui.visuals().error_fg_color;
        let mut jump = None;
        for diagnostic in diagnostics.iter().take(MAX_LISTED_ERRORS) {
            let text =
                egui::RichText::new(format!("Line {}: {}", diagnostic.line, diagnostic.message))
                    .color(color);
            if ui
                .add(
                    egui::Label::new(text)
                        .truncate()
                        .sense(egui::Sense::click()),
                )
                .on_hover_text("Go to the error")
                .clicked()
            {
                jump = Some(diagnostic.range.start);
            }
        }
        if diagnostics.len() > MAX_LISTED_ERRORS {
            ui.weak(format!(
                "and {} more",
                diagnostics.len() - MAX_LISTED_ERRORS
            ));
        }
        if jump.is_some() {
            self.reveal = jump;
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_endings_and_byte_order_mark_survive_a_round_trip() {
        for original in [
            "cube(1);\nsphere(2);\n",
            "cube(1);\r\nsphere(2);\r\n",
            "\u{feff}cube(1);\r\n// è\r\n",
        ] {
            let decoded = decode(original.as_bytes().to_vec());
            assert!(!decoded.text.contains('\r'));
            assert!(!decoded.text.starts_with('\u{feff}'));
            assert_eq!(decoded.read_only, None);
            assert_eq!(decoded.format.encode(&decoded.text), original);
        }
    }

    #[test]
    fn non_utf8_files_are_read_only() {
        // "è" in Windows-1252.
        let decoded = decode(b"// caff\xe8\ncube(1);\n".to_vec());
        assert!(decoded.read_only.is_some());
        assert!(decoded.text.ends_with("cube(1);\n"));
    }

    #[test]
    fn parse_errors_are_located_and_never_empty() {
        let text = "cube(1);\nsphere(2)\n";
        let diagnostics = diagnostics_of(text);
        assert!(!diagnostics.is_empty());
        for d in &diagnostics {
            assert!(!d.range.is_empty(), "{d:?}");
            assert!(d.range.end <= text.len());
        }
        assert_eq!(diagnostics_of("cube(1);\n"), Vec::new());
    }

    #[test]
    fn highlighting_covers_the_text_exactly() {
        let text = "module box(s) { cube(s); } // è\nbox($fn = 3);\n\"oops";
        let picked = Some(16..24);
        let diagnostics = diagnostics_of(text);
        let job = build_job(
            text,
            &diagnostics,
            picked,
            &FontId::monospace(12.0),
            |role| match role {
                Role::Plain => Color32::WHITE,
                _ => Color32::RED,
            },
            Stroke::new(1.0, Color32::RED),
        );
        let mut offset = 0;
        for section in &job.sections {
            assert_eq!(section.byte_range.start.0, offset);
            assert!(text.is_char_boundary(section.byte_range.end.0));
            offset = section.byte_range.end.0;
        }
        assert_eq!(offset, text.len());
        let picked_text: String = job
            .sections
            .iter()
            .filter(|s| s.format.background == PICKED_CODE)
            .map(|s| &text[s.byte_range.start.0..s.byte_range.end.0])
            .collect();
        assert_eq!(picked_text, "cube(s);");
        let role_at = |needle: &str| {
            let at = text.find(needle).unwrap();
            job.sections
                .iter()
                .find(|s| s.byte_range.start.0 == at)
                .map(|s| s.format.color)
        };
        assert_eq!(role_at("box("), Some(Color32::RED)); // definition
        assert_eq!(role_at("s)"), Some(Color32::WHITE));
    }

    #[test]
    fn offsets_convert_to_characters() {
        let text = "// è\ncube(1);";
        let cube = text.find("cube").unwrap();
        assert_eq!(char_index(text, cube), 5);
        assert_eq!(line_of(text, cube), 2);
        assert_eq!(floor_boundary(text, 4), 3);
    }
}
