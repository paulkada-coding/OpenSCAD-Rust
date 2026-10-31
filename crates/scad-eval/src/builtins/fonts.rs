//! Font discovery, shaping, and glyph outline metrics for `fontmetrics()` and `textmetrics()`.
//!
//! Matches FreeType + HarfBuzz scaling at `char_size = 100_000` (26.6 fixed-point points)
//! and `100` DPI, scaled to the requested OpenSCAD `size`.

use std::str::FromStr;
use std::sync::LazyLock;

use scad_syntax::ast::Span;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{
    DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target,
};
use skrifa::raw::TableProvider;
use skrifa::string::StringId;
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::eval::Evaluator;
use crate::value::{Arguments, Value};

const LIBERATION_REGULAR: &[u8] =
    include_bytes!("../../fonts/Liberation-2.00.1/ttf/LiberationSans-Regular.ttf");
const LIBERATION_BOLD: &[u8] =
    include_bytes!("../../fonts/Liberation-2.00.1/ttf/LiberationSans-Bold.ttf");
const LIBERATION_ITALIC: &[u8] =
    include_bytes!("../../fonts/Liberation-2.00.1/ttf/LiberationSans-Italic.ttf");
const LIBERATION_BOLD_ITALIC: &[u8] =
    include_bytes!("../../fonts/Liberation-2.00.1/ttf/LiberationSans-BoldItalic.ttf");

/// Shared font database with bundled Liberation Sans (SIL OFL 1.1) and system fallback fonts.
static FONT_DB: LazyLock<fontdb::Database> = LazyLock::new(|| {
    let mut db = fontdb::Database::new();
    for data in [
        LIBERATION_REGULAR,
        LIBERATION_BOLD,
        LIBERATION_ITALIC,
        LIBERATION_BOLD_ITALIC,
    ] {
        db.load_font_data(data.to_vec());
    }
    db.load_system_fonts();
    db
});

/// FreeType `FT_Set_Char_Size(face, 100_000, 100_000, 100, 100)`:
/// `dim_26_6 = (100_000 * 100 + 36) / 72 = 138_889` (26.6 fractional pixels).
const FT_CHAR_SIZE_26_6: f64 = 100_000.0;
const FT_DIM_26_6: i64 = (100_000 * 100 + 36) / 72;
const FT_PPEM: f32 = ((FT_DIM_26_6 + 32) >> 6) as f32;

/// FreeType's `FT_Request_Metrics` 16.16 fixed-point scale from font units to 26.6 pixels.
fn ft_scale_16_16(upem: u16) -> i64 {
    let upem = i64::from(upem.max(1));
    (FT_DIM_26_6 * 65536 + upem / 2) / upem
}

/// FreeType's `FT_MulFix(a, b)` signed-magnitude 16.16 fixed-point multiplication.
fn ft_mul_fix(a: i64, b_16_16: i64) -> i64 {
    let prod = i128::from(a) * i128::from(b_16_16);
    if prod >= 0 {
        ((prod + 0x8000) >> 16) as i64
    } else {
        -((-prod + 0x8000) >> 16) as i64
    }
}

fn clean_zero(x: f64) -> f64 {
    x + 0.0
}

/// Bind positional and named arguments following OpenSCAD parameter order.
struct BoundArgs<'a> {
    bound: Vec<(&'static str, &'a Value)>,
}

impl<'a> BoundArgs<'a> {
    fn new(args: &'a Arguments, names: &'static [&'static str]) -> Self {
        let mut bound: Vec<(&'static str, &'a Value)> = Vec::new();
        for (name, val) in &args.named {
            if let Some(&canon) = names.iter().find(|&&p| p == name.as_str()) {
                bound.push((canon, val));
            }
        }
        let mut pos = args.positional.iter();
        for &param in names {
            if bound.iter().any(|(n, _)| *n == param) {
                continue;
            }
            if let Some(val) = pos.next() {
                bound.push((param, val));
            }
        }
        Self { bound }
    }

    fn string(
        &self,
        ev: &mut Evaluator,
        fn_name: &str,
        param: &str,
        default: &str,
        span: Span,
    ) -> String {
        match self
            .bound
            .iter()
            .find(|(n, _)| *n == param)
            .map(|(_, v)| *v)
        {
            None | Some(Value::Undef) => default.to_string(),
            Some(Value::String(s)) => s.to_string(),
            Some(other) => {
                ev.warning(
                    format!(
                        "{fn_name}(..., {param}={}) Invalid type: expected string, found {}",
                        other.to_repr(),
                        other.type_name()
                    ),
                    span,
                );
                default.to_string()
            }
        }
    }

    fn number(
        &self,
        ev: &mut Evaluator,
        fn_name: &str,
        param: &str,
        default: f64,
        span: Span,
    ) -> f64 {
        match self
            .bound
            .iter()
            .find(|(n, _)| *n == param)
            .map(|(_, v)| *v)
        {
            None | Some(Value::Undef) => default,
            Some(Value::Number(n)) if n.is_finite() => *n,
            Some(Value::Number(_)) => default,
            Some(other) => {
                ev.warning(
                    format!(
                        "{fn_name}(..., {param}={}) Invalid type: expected number, found {}",
                        other.to_repr(),
                        other.type_name()
                    ),
                    span,
                );
                default
            }
        }
    }
}

struct FontQuerySpec {
    families: Vec<String>,
    weight: fontdb::Weight,
    style: fontdb::Style,
    stretch: fontdb::Stretch,
}

fn parse_font_spec(spec: &str) -> FontQuerySpec {
    let mut parts = spec.split(':');
    let family_part = parts.next().unwrap_or("").trim();
    let mut families: Vec<String> = family_part
        .split(',')
        .map(|s| s.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if families.is_empty() {
        families.push("Liberation Sans".to_string());
    }

    let mut weight = fontdb::Weight::NORMAL;
    let mut style = fontdb::Style::Normal;
    let mut stretch = fontdb::Stretch::Normal;

    for raw_attr in parts {
        let attr = raw_attr.trim();
        let val = match attr.split_once('=') {
            Some((k, v)) => {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim();
                match k.as_str() {
                    "style" | "weight" | "slant" | "width" => v,
                    _ => continue,
                }
            }
            None => attr,
        };
        apply_style_tokens(val, &mut weight, &mut style, &mut stretch);
    }

    FontQuerySpec {
        families,
        weight,
        style,
        stretch,
    }
}

fn apply_style_tokens(
    tokens: &str,
    weight: &mut fontdb::Weight,
    style: &mut fontdb::Style,
    stretch: &mut fontdb::Stretch,
) {
    let lower = tokens.to_ascii_lowercase();
    for word in lower.split(|c: char| c.is_whitespace() || c == '-' || c == '_') {
        match word {
            "thin" | "hairline" => *weight = fontdb::Weight::THIN,
            "extralight" | "ultralight" => *weight = fontdb::Weight::EXTRA_LIGHT,
            "light" => *weight = fontdb::Weight::LIGHT,
            "regular" | "normal" | "book" | "roman" => *weight = fontdb::Weight::NORMAL,
            "medium" => *weight = fontdb::Weight::MEDIUM,
            "semibold" | "demibold" | "demi" => *weight = fontdb::Weight::SEMIBOLD,
            "bold" => *weight = fontdb::Weight::BOLD,
            "extrabold" | "ultrabold" => *weight = fontdb::Weight::EXTRA_BOLD,
            "black" | "heavy" => *weight = fontdb::Weight::BLACK,
            "italic" => *style = fontdb::Style::Italic,
            "oblique" => *style = fontdb::Style::Oblique,
            "condensed" | "narrow" => *stretch = fontdb::Stretch::Condensed,
            "expanded" | "wide" => *stretch = fontdb::Stretch::Expanded,
            _ => {}
        }
    }
}

fn resolve_face(spec: &str) -> fontdb::ID {
    let db = &*FONT_DB;
    let parsed = parse_font_spec(spec);
    let family_refs: Vec<fontdb::Family<'_>> = parsed
        .families
        .iter()
        .map(|s| match s.to_ascii_lowercase().as_str() {
            "sans" | "sans-serif" => fontdb::Family::SansSerif,
            "serif" => fontdb::Family::Serif,
            "mono" | "monospace" => fontdb::Family::Monospace,
            "cursive" => fontdb::Family::Cursive,
            "fantasy" => fontdb::Family::Fantasy,
            _ => fontdb::Family::Name(s.as_str()),
        })
        .collect();

    if let Some(id) = db.query(&fontdb::Query {
        families: &family_refs,
        weight: parsed.weight,
        stretch: parsed.stretch,
        style: parsed.style,
    }) {
        return id;
    }

    db.query(&fontdb::Query {
        families: &[fontdb::Family::Name("Liberation Sans")],
        weight: parsed.weight,
        stretch: parsed.stretch,
        style: parsed.style,
    })
    .or_else(|| {
        db.query(&fontdb::Query {
            families: &[fontdb::Family::Name("Liberation Sans")],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        })
    })
    .expect("bundled Liberation Sans must always be present in FONT_DB")
}

fn localized_name(font: &FontRef<'_>, id: StringId) -> Option<String> {
    let strings = font.localized_strings(id);
    strings
        .english_or_first()
        .map(|s| s.chars().collect::<String>())
        .filter(|s| !s.is_empty())
}

/// Evaluates `fontmetrics([size[, font]])`.
pub fn fontmetrics(ev: &mut Evaluator, args: &Arguments, span: Span) -> Value {
    let bound = BoundArgs::new(args, &["size", "font"]);
    let size = bound.number(ev, "fontmetrics", "size", 10.0, span);
    let font_spec = bound.string(ev, "fontmetrics", "font", "", span);
    let id = resolve_face(&font_spec);

    FONT_DB
        .with_face_data(id, |data, face_index| {
            let Ok(font) = FontRef::from_index(data, face_index) else {
                return Value::Undef;
            };
            let head = font.head().ok();
            let hhea = font.hhea().ok();
            let upem = head.as_ref().map_or(2048, |h| h.units_per_em());
            let scale_16_16 = ft_scale_16_16(upem);
            let out_scale = size / FT_CHAR_SIZE_26_6;

            let ascender = hhea
                .as_ref()
                .map_or(0, |h| i64::from(h.ascender().to_i16()));
            let descender = hhea
                .as_ref()
                .map_or(0, |h| i64::from(h.descender().to_i16()));
            let line_gap = hhea
                .as_ref()
                .map_or(0, |h| i64::from(h.line_gap().to_i16()));
            let height = ascender - descender + line_gap;

            let y_max = head.as_ref().map_or(0, |h| i64::from(h.y_max()));
            let y_min = head.as_ref().map_or(0, |h| i64::from(h.y_min()));

            let nominal_ascent = clean_zero(ft_mul_fix(ascender, scale_16_16) as f64 * out_scale);
            let nominal_descent = clean_zero(ft_mul_fix(descender, scale_16_16) as f64 * out_scale);
            let max_ascent = clean_zero(ft_mul_fix(y_max, scale_16_16) as f64 * out_scale);
            let max_descent = clean_zero(ft_mul_fix(y_min, scale_16_16) as f64 * out_scale);
            let interline = clean_zero(ft_mul_fix(height, scale_16_16) as f64 * out_scale);

            let family = localized_name(&font, StringId::TYPOGRAPHIC_FAMILY_NAME)
                .or_else(|| localized_name(&font, StringId::FAMILY_NAME))
                .unwrap_or_else(|| "Liberation Sans".to_string());
            let style = localized_name(&font, StringId::TYPOGRAPHIC_SUBFAMILY_NAME)
                .or_else(|| localized_name(&font, StringId::SUBFAMILY_NAME))
                .unwrap_or_else(|| "Regular".to_string());

            Value::object(vec![
                (
                    "nominal".into(),
                    Value::object(vec![
                        ("ascent".into(), Value::Number(nominal_ascent)),
                        ("descent".into(), Value::Number(nominal_descent)),
                    ]),
                ),
                (
                    "max".into(),
                    Value::object(vec![
                        ("ascent".into(), Value::Number(max_ascent)),
                        ("descent".into(), Value::Number(max_descent)),
                    ]),
                ),
                ("interline".into(), Value::Number(interline)),
                (
                    "font".into(),
                    Value::object(vec![
                        ("family".into(), Value::from(family.as_str())),
                        ("style".into(), Value::from(style.as_str())),
                    ]),
                ),
            ])
        })
        .unwrap_or(Value::Undef)
}

#[derive(Default)]
struct CBoxPen {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    has_points: bool,
}

impl CBoxPen {
    fn new() -> Self {
        Self {
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
            has_points: false,
        }
    }

    fn record(&mut self, x: f32, y: f32) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
        self.has_points = true;
    }

    /// Returns grid-fitted `(min_x, min_y, max_x, max_y)` in 26.6 fractional pixels,
    /// matching FreeType's `FT_Glyph_Get_CBox(..., FT_GLYPH_BBOX_GRIDFIT)`.
    fn finish_26_6(&self) -> Option<(f64, f64, f64, f64)> {
        self.has_points.then(|| {
            let x0 = (f64::from(self.min_x) * 64.0).round() / 64.0;
            let y0 = (f64::from(self.min_y) * 64.0).round() / 64.0;
            let x1 = (f64::from(self.max_x) * 64.0).round() / 64.0;
            let y1 = (f64::from(self.max_y) * 64.0).round() / 64.0;
            (
                x0.floor() * 64.0,
                y0.floor() * 64.0,
                x1.ceil() * 64.0,
                y1.ceil() * 64.0,
            )
        })
    }
}

impl OutlinePen for CBoxPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.record(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.record(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.record(cx0, cy0);
        self.record(x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.record(cx0, cy0);
        self.record(cx1, cy1);
        self.record(x, y);
    }

    fn close(&mut self) {}
}

fn zero_textmetrics() -> Value {
    Value::object(vec![
        ("position".into(), Value::list(vec![0.0.into(), 0.0.into()])),
        ("size".into(), Value::list(vec![0.0.into(), 0.0.into()])),
        ("ascent".into(), Value::Number(0.0)),
        ("descent".into(), Value::Number(0.0)),
        ("offset".into(), Value::list(vec![0.0.into(), 0.0.into()])),
        ("advance".into(), Value::list(vec![0.0.into(), 0.0.into()])),
    ])
}

/// Evaluates `textmetrics(text[, size, font, halign, valign, spacing, direction, language, script])`.
pub fn textmetrics(ev: &mut Evaluator, args: &Arguments, span: Span) -> Value {
    let bound = BoundArgs::new(
        args,
        &[
            "text",
            "size",
            "font",
            "halign",
            "valign",
            "spacing",
            "direction",
            "language",
            "script",
        ],
    );
    let text = bound.string(ev, "textmetrics", "text", "", span);
    let size = bound.number(ev, "textmetrics", "size", 10.0, span);
    let font_spec = bound.string(ev, "textmetrics", "font", "", span);
    let halign = bound.string(ev, "textmetrics", "halign", "default", span);
    let valign = bound.string(ev, "textmetrics", "valign", "default", span);
    let spacing = bound.number(ev, "textmetrics", "spacing", 1.0, span);
    let direction_str = bound.string(ev, "textmetrics", "direction", "", span);
    let language_str = bound.string(ev, "textmetrics", "language", "en", span);
    let script_str = bound.string(ev, "textmetrics", "script", "", span);

    if text.is_empty() || size <= 0.0 {
        return zero_textmetrics();
    }

    let id = resolve_face(&font_spec);
    FONT_DB
        .with_face_data(id, |data, face_index| {
            shape_and_measure(
                data,
                face_index,
                &text,
                size,
                &halign,
                &valign,
                spacing,
                &direction_str,
                &language_str,
                &script_str,
            )
        })
        .flatten()
        .unwrap_or_else(zero_textmetrics)
}

#[allow(clippy::too_many_arguments)]
fn shape_and_measure(
    data: &[u8],
    face_index: u32,
    text: &str,
    size: f64,
    halign: &str,
    valign: &str,
    spacing: f64,
    direction_str: &str,
    language_str: &str,
    script_str: &str,
) -> Option<Value> {
    let font = FontRef::from_index(data, face_index).ok()?;
    let upem = font.head().ok().map_or(2048, |h| h.units_per_em());
    let scale_16_16 = ft_scale_16_16(upem);

    let hb_face = rustybuzz::Face::from_slice(data, face_index)?;

    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);

    if !language_str.is_empty()
        && let Ok(lang) = rustybuzz::Language::from_str(language_str)
    {
        buffer.set_language(lang);
    }

    if !script_str.is_empty() {
        let mut bytes = [b' '; 4];
        for (i, b) in script_str.bytes().take(4).enumerate() {
            bytes[i] = if i == 0 {
                b.to_ascii_uppercase()
            } else {
                b.to_ascii_lowercase()
            };
        }
        if let Some(script) =
            rustybuzz::Script::from_iso15924_tag(rustybuzz::ttf_parser::Tag::from_bytes(&bytes))
        {
            buffer.set_script(script);
        }
    }

    let explicit_dir = match direction_str {
        "ltr" => Some(rustybuzz::Direction::LeftToRight),
        "rtl" => Some(rustybuzz::Direction::RightToLeft),
        "ttb" => Some(rustybuzz::Direction::TopToBottom),
        "btt" => Some(rustybuzz::Direction::BottomToTop),
        _ => None,
    };
    if let Some(dir) = explicit_dir {
        buffer.set_direction(dir);
    } else {
        buffer.guess_segment_properties();
    }
    let dir = buffer.direction();
    let is_vertical = matches!(
        dir,
        rustybuzz::Direction::TopToBottom | rustybuzz::Direction::BottomToTop
    );

    let glyph_buffer = rustybuzz::shape(&hb_face, &[], buffer);
    let infos = glyph_buffer.glyph_infos();
    let positions = glyph_buffer.glyph_positions();

    let has_vmtx = font.vmtx().is_ok();
    let unscaled_metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let synth_v_adv = if is_vertical && !has_vmtx {
        font.os2()
            .ok()
            .map(|os2| i64::from(os2.s_typo_ascender()) - i64::from(os2.s_typo_descender()))
            .or_else(|| {
                font.hhea()
                    .ok()
                    .map(|h| i64::from(h.ascender().to_i16()) - i64::from(h.descender().to_i16()))
            })
    } else {
        None
    };

    let outlines = font.outline_glyphs();
    let glyph_size = Size::new(FT_PPEM);
    let hint_opts = HintingOptions {
        engine: Engine::AutoFallback,
        target: Target::Smooth {
            mode: SmoothMode::Normal,
            symmetric_rendering: false,
            preserve_linear_metrics: false,
        },
    };
    let hinter =
        HintingInstance::new(&outlines, glyph_size, LocationRef::default(), hint_opts).ok();

    let mut cursor_x = 0.0_f64;
    let mut cursor_y = 0.0_f64;
    let mut bbox_min_x = f64::INFINITY;
    let mut bbox_min_y = f64::INFINITY;
    let mut bbox_max_x = f64::NEG_INFINITY;
    let mut bbox_max_y = f64::NEG_INFINITY;
    let mut max_glyph_top = 0.0_f64;
    let mut min_glyph_bottom = 0.0_f64;
    let mut has_ink = false;

    for (info, pos) in infos.iter().zip(positions.iter()) {
        let gid = GlyphId::new(info.glyph_id);
        let (x_off, y_off, x_adv, y_adv) = if let Some(v_adv_design) = synth_v_adv {
            let h_adv_design = unscaled_metrics
                .advance_width(gid)
                .map_or(0, |w| w.round() as i64);
            let (gy_min_d, gy_max_d) = unscaled_metrics
                .bounds(gid)
                .map_or((0, 0), |b| (b.y_min.round() as i64, b.y_max.round() as i64));
            let v_orig_y_design = (v_adv_design + gy_min_d + gy_max_d) / 2;
            let x_off = -(ft_mul_fix(h_adv_design, scale_16_16) >> 1) as f64;
            let y_off = -ft_mul_fix(v_orig_y_design, scale_16_16) as f64;
            let y_adv = -ft_mul_fix(v_adv_design, scale_16_16) as f64;
            (x_off, y_off, 0.0, y_adv)
        } else {
            (
                ft_mul_fix(i64::from(pos.x_offset), scale_16_16) as f64,
                ft_mul_fix(i64::from(pos.y_offset), scale_16_16) as f64,
                ft_mul_fix(i64::from(pos.x_advance), scale_16_16) as f64,
                ft_mul_fix(i64::from(pos.y_advance), scale_16_16) as f64,
            )
        };
        if let Some(glyph) = outlines.get(gid) {
            let mut pen = CBoxPen::new();
            let settings = match &hinter {
                Some(h) if h.is_enabled() => DrawSettings::hinted(h, false),
                _ => DrawSettings::unhinted(glyph_size, LocationRef::default()),
            };
            if glyph.draw(settings, &mut pen).is_ok()
                && let Some((gx0, gy0, gx1, gy1)) = pen.finish_26_6()
            {
                let x0 = cursor_x + x_off + gx0;
                let x1 = cursor_x + x_off + gx1;
                let y0 = cursor_y + y_off + gy0;
                let y1 = cursor_y + y_off + gy1;
                bbox_min_x = bbox_min_x.min(x0);
                bbox_max_x = bbox_max_x.max(x1);
                bbox_min_y = bbox_min_y.min(y0);
                bbox_max_y = bbox_max_y.max(y1);

                if is_vertical {
                    max_glyph_top = max_glyph_top.max(gy1);
                    min_glyph_bottom = min_glyph_bottom.min(gy0);
                } else {
                    max_glyph_top = max_glyph_top.max(y1);
                    min_glyph_bottom = min_glyph_bottom.min(y0);
                }
                has_ink = true;
            }
        }
        cursor_x += x_adv * spacing;
        cursor_y += y_adv * spacing;
    }

    let out_scale = size / FT_CHAR_SIZE_26_6;
    let adv_x = clean_zero(cursor_x * out_scale);
    let adv_y = clean_zero(cursor_y * out_scale);

    if !has_ink {
        return Some(Value::object(vec![
            ("position".into(), Value::list(vec![0.0.into(), 0.0.into()])),
            ("size".into(), Value::list(vec![0.0.into(), 0.0.into()])),
            ("ascent".into(), Value::Number(0.0)),
            ("descent".into(), Value::Number(0.0)),
            ("offset".into(), Value::list(vec![0.0.into(), 0.0.into()])),
            (
                "advance".into(),
                Value::list(vec![adv_x.into(), adv_y.into()]),
            ),
        ]));
    }

    let raw_min_x = bbox_min_x * out_scale;
    let raw_max_x = bbox_max_x * out_scale;
    let raw_min_y = bbox_min_y * out_scale;
    let raw_max_y = bbox_max_y * out_scale;
    let ascent = clean_zero(max_glyph_top * out_scale);
    let descent = clean_zero(min_glyph_bottom * out_scale);

    let offset_x = if is_vertical {
        match halign {
            "left" => -raw_min_x,
            "right" => -raw_max_x,
            _ => 0.0,
        }
    } else {
        match halign {
            "center" => -adv_x / 2.0,
            "right" => -adv_x,
            _ => 0.0,
        }
    };

    let offset_y = if is_vertical {
        match valign {
            "center" => -adv_y / 2.0,
            "bottom" => -adv_y,
            _ => 0.0,
        }
    } else {
        match valign {
            "top" => -ascent,
            "center" => -(ascent + descent) / 2.0,
            "bottom" => -descent,
            _ => 0.0,
        }
    };

    let pos_x = clean_zero(raw_min_x + offset_x);
    let pos_y = clean_zero(raw_min_y + offset_y);
    let size_x = clean_zero(raw_max_x - raw_min_x);
    let size_y = clean_zero(raw_max_y - raw_min_y);
    let offset_x = clean_zero(offset_x);
    let offset_y = clean_zero(offset_y);

    Some(Value::object(vec![
        (
            "position".into(),
            Value::list(vec![pos_x.into(), pos_y.into()]),
        ),
        (
            "size".into(),
            Value::list(vec![size_x.into(), size_y.into()]),
        ),
        ("ascent".into(), Value::Number(ascent)),
        ("descent".into(), Value::Number(descent)),
        (
            "offset".into(),
            Value::list(vec![offset_x.into(), offset_y.into()]),
        ),
        (
            "advance".into(),
            Value::list(vec![adv_x.into(), adv_y.into()]),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{EvalOptions, Evaluator};

    fn run_logs(src: &str) -> Vec<String> {
        let mut ev = Evaluator::new(EvalOptions::default());
        let (_, err) = ev.run(Path::new("test.scad"), src);
        assert!(err.is_none());
        ev.messages().iter().map(|m| m.text.clone()).collect()
    }

    #[test]
    fn matches_openscad_textmetrics_and_fontmetrics() {
        let logs = run_logs(
            r#"
            echo(textmetrics("a"));
            echo(textmetrics(""));
            echo(fontmetrics());
            echo(textmetrics("ab", size=10));
            echo(textmetrics("Hi", size = 20, font = "Liberation Sans:style=Bold", halign = "center", valign = "center", spacing = 1.5, direction = "ttb"));
            echo(fontmetrics(size = 20, font = "Liberation Sans:style=Bold"));
            echo(textmetrics("gp", size = 10, halign = "center", valign = "center"));
            echo(textmetrics("   ", size = 10));
            "#,
        );
        assert_eq!(
            logs[0],
            "{ position = [0.5888, -0.1408]; size = [7.136, 7.616]; ascent = 7.4752; descent = -0.1408; offset = [0, 0]; advance = [7.7243, 0]; }"
        );
        assert_eq!(
            logs[1],
            "{ position = [0, 0]; size = [0, 0]; ascent = 0; descent = 0; offset = [0, 0]; advance = [0, 0]; }"
        );
        assert_eq!(
            logs[2],
            r#"{ nominal = { ascent = 12.5733; descent = -2.9433; }; max = { ascent = 13.6109; descent = -4.2114; }; interline = 15.9709; font = { family = "Liberation Sans"; style = "Regular"; }; }"#
        );
        assert_eq!(
            logs[3],
            "{ position = [0.5888, -0.1408]; size = [14.2779, 10.208]; ascent = 10.0672; descent = -0.1408; offset = [0, 0]; advance = [15.4486, 0]; }"
        );
        assert_eq!(
            logs[4],
            "{ position = [-8.174, -23.0984]; size = [16.3328, 58.729]; ascent = 20.1344; descent = 0; offset = [0, 39.1032]; advance = [0, -78.2064]; }"
        );
        assert_eq!(
            logs[5],
            r#"{ nominal = { ascent = 25.1466; descent = -5.8866; }; max = { ascent = 28.7002; descent = -10.4574; }; interline = 31.9418; font = { family = "Liberation Sans"; style = "Bold"; }; }"#
        );
        assert_eq!(
            logs[6],
            "{ position = [-7.1419, -5.1776]; size = [14.2843, 10.3552]; ascent = 7.4688; descent = -2.8864; offset = [-7.7243, -2.2912]; advance = [15.4486, 0]; }"
        );
        assert_eq!(
            logs[7],
            "{ position = [0, 0]; size = [0, 0]; ascent = 0; descent = 0; offset = [0, 0]; advance = [11.5764, 0]; }"
        );
    }
}
