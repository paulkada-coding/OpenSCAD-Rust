//! Shared font discovery, text shaping, metrics, and 2D glyph contour extraction.
//!
//! Used by `scad-eval` for `fontmetrics()` / `textmetrics()` and ready for `geometry`
//! to tessellate `text()` nodes into 2D `CrossSection` polygons in Milestone 5.
//!
//! Matches FreeType + HarfBuzz scaling at `char_size = 100_000` (26.6 fixed-point points)
//! and `100` DPI, scaled to the requested OpenSCAD `size`.

use std::str::FromStr;
use std::sync::LazyLock;

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{
    DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target,
};
use skrifa::raw::TableProvider;
use skrifa::string::StringId;
use skrifa::{FontRef, GlyphId, MetadataProvider};

const LIBERATION_REGULAR: &[u8] =
    include_bytes!("../fonts/Liberation-2.00.1/ttf/LiberationSans-Regular.ttf");
const LIBERATION_BOLD: &[u8] =
    include_bytes!("../fonts/Liberation-2.00.1/ttf/LiberationSans-Bold.ttf");
const LIBERATION_ITALIC: &[u8] =
    include_bytes!("../fonts/Liberation-2.00.1/ttf/LiberationSans-Italic.ttf");
const LIBERATION_BOLD_ITALIC: &[u8] =
    include_bytes!("../fonts/Liberation-2.00.1/ttf/LiberationSans-BoldItalic.ttf");

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

/// Metrics returned by `fontmetrics(size, font)`.
#[derive(Debug, Clone, PartialEq)]
pub struct FontMetrics {
    pub nominal_ascent: f64,
    pub nominal_descent: f64,
    pub max_ascent: f64,
    pub max_descent: f64,
    pub interline: f64,
    pub family: String,
    pub style: String,
}

/// Parameters for `text()` and `textmetrics()`.
#[derive(Debug, Clone)]
pub struct TextParams<'a> {
    pub text: &'a str,
    pub size: f64,
    pub font: &'a str,
    pub halign: &'a str,
    pub valign: &'a str,
    pub spacing: f64,
    pub direction: &'a str,
    pub language: &'a str,
    pub script: &'a str,
}

impl Default for TextParams<'static> {
    fn default() -> Self {
        Self {
            text: "",
            size: 10.0,
            font: "",
            halign: "default",
            valign: "default",
            spacing: 1.0,
            direction: "",
            language: "en",
            script: "",
        }
    }
}

/// Metrics returned by `textmetrics(...)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    pub position: [f64; 2],
    pub size: [f64; 2],
    pub ascent: f64,
    pub descent: f64,
    pub offset: [f64; 2],
    pub advance: [f64; 2],
}

impl Default for TextMetrics {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0],
            size: [0.0, 0.0],
            ascent: 0.0,
            descent: 0.0,
            offset: [0.0, 0.0],
            advance: [0.0, 0.0],
        }
    }
}

fn ft_scale_16_16(upem: u16) -> i64 {
    let upem = i64::from(upem.max(1));
    (FT_DIM_26_6 * 65536 + upem / 2) / upem
}

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

/// Computes font-level metrics matching OpenSCAD's `fontmetrics(size, font)`.
pub fn measure_font(size: f64, font_spec: &str) -> Option<FontMetrics> {
    let id = resolve_face(font_spec);
    FONT_DB
        .with_face_data(id, |data, face_index| {
            let font = FontRef::from_index(data, face_index).ok()?;
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

            Some(FontMetrics {
                nominal_ascent,
                nominal_descent,
                max_ascent,
                max_descent,
                interline,
                family,
                style,
            })
        })
        .flatten()
}

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

struct PositionedGlyph {
    gid: GlyphId,
    /// Glyph origin in 26.6 fractional pixels before overall alignment `offset`.
    origin_26_6: [f64; 2],
}

struct ShapedLayout {
    metrics: TextMetrics,
    glyphs: Vec<PositionedGlyph>,
}

/// Computes shaped text metrics matching OpenSCAD's `textmetrics(...)`.
pub fn measure_text(params: &TextParams<'_>) -> TextMetrics {
    if params.text.is_empty() || params.size <= 0.0 {
        return TextMetrics::default();
    }
    let id = resolve_face(params.font);
    FONT_DB
        .with_face_data(id, |data, face_index| {
            shape_layout(data, face_index, params).map(|s| s.metrics)
        })
        .flatten()
        .unwrap_or_default()
}

/// Extracts flattened 2D polygon contours (each a closed ring `[x, y]`) in final
/// OpenSCAD coordinates (including `halign`/`valign` alignment offset).
///
/// Curves are subdivided recursively until chord deviation is at most `max_chord_error`.
pub fn shape_contours(params: &TextParams<'_>, max_chord_error: f64) -> Vec<Vec<[f64; 2]>> {
    contours_with(params, Flatten::Tolerance(max_chord_error.max(1e-4)))
}

/// Like [`shape_contours`], but every curve is split into `steps_per_curve`
/// equal parameter steps (OpenSCAD's `text()` uses a fixed count per curve).
pub fn shape_contours_steps(params: &TextParams<'_>, steps_per_curve: u32) -> Vec<Vec<[f64; 2]>> {
    contours_with(params, Flatten::Steps(steps_per_curve.max(1)))
}

/// How [`FlattenPen`] approximates curves.
#[derive(Clone, Copy)]
enum Flatten {
    Tolerance(f64),
    Steps(u32),
}

fn contours_with(params: &TextParams<'_>, mode: Flatten) -> Vec<Vec<[f64; 2]>> {
    if params.text.is_empty() || params.size <= 0.0 {
        return Vec::new();
    }
    let id = resolve_face(params.font);
    FONT_DB
        .with_face_data(id, |data, face_index| {
            let layout = shape_layout(data, face_index, params)?;
            let font = FontRef::from_index(data, face_index).ok()?;
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
            let out_scale = params.size / FT_CHAR_SIZE_26_6;
            let [off_x, off_y] = layout.metrics.offset;

            let mut all_contours = Vec::new();
            for pg in &layout.glyphs {
                if let Some(glyph) = outlines.get(pg.gid) {
                    let settings = match &hinter {
                        Some(h) if h.is_enabled() => DrawSettings::hinted(h, false),
                        _ => DrawSettings::unhinted(glyph_size, LocationRef::default()),
                    };
                    let mut pen = FlattenPen::new(
                        [
                            pg.origin_26_6[0] * out_scale + off_x,
                            pg.origin_26_6[1] * out_scale + off_y,
                        ],
                        64.0 * out_scale,
                        mode,
                    );
                    if glyph.draw(settings, &mut pen).is_ok() {
                        all_contours.extend(pen.finish());
                    }
                }
            }
            Some(all_contours)
        })
        .flatten()
        .unwrap_or_default()
}

fn shape_layout(data: &[u8], face_index: u32, params: &TextParams<'_>) -> Option<ShapedLayout> {
    let font = FontRef::from_index(data, face_index).ok()?;
    let upem = font.head().ok().map_or(2048, |h| h.units_per_em());
    let scale_16_16 = ft_scale_16_16(upem);

    let hb_face = rustybuzz::Face::from_slice(data, face_index)?;
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(params.text);

    if !params.language.is_empty()
        && let Ok(lang) = rustybuzz::Language::from_str(params.language)
    {
        buffer.set_language(lang);
    }

    if !params.script.is_empty() {
        let mut bytes = [b' '; 4];
        for (i, b) in params.script.bytes().take(4).enumerate() {
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

    let explicit_dir = match params.direction {
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
    let mut shaped_glyphs = Vec::with_capacity(infos.len());

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

        shaped_glyphs.push(PositionedGlyph {
            gid,
            origin_26_6: [cursor_x + x_off, cursor_y + y_off],
        });

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
        cursor_x += x_adv * params.spacing;
        cursor_y += y_adv * params.spacing;
    }

    let out_scale = params.size / FT_CHAR_SIZE_26_6;
    let adv_x = clean_zero(cursor_x * out_scale);
    let adv_y = clean_zero(cursor_y * out_scale);

    if !has_ink {
        return Some(ShapedLayout {
            metrics: TextMetrics {
                advance: [adv_x, adv_y],
                ..TextMetrics::default()
            },
            glyphs: Vec::new(),
        });
    }

    let raw_min_x = bbox_min_x * out_scale;
    let raw_max_x = bbox_max_x * out_scale;
    let raw_min_y = bbox_min_y * out_scale;
    let raw_max_y = bbox_max_y * out_scale;
    let ascent = clean_zero(max_glyph_top * out_scale);
    let descent = clean_zero(min_glyph_bottom * out_scale);

    let offset_x = if is_vertical {
        match params.halign {
            "left" => -raw_min_x,
            "right" => -raw_max_x,
            _ => 0.0,
        }
    } else {
        match params.halign {
            "center" => -adv_x / 2.0,
            "right" => -adv_x,
            _ => 0.0,
        }
    };

    let offset_y = if is_vertical {
        match params.valign {
            "center" => -adv_y / 2.0,
            "bottom" => -adv_y,
            _ => 0.0,
        }
    } else {
        match params.valign {
            "top" => -ascent,
            "center" => -(ascent + descent) / 2.0,
            "bottom" => -descent,
            _ => 0.0,
        }
    };

    let metrics = TextMetrics {
        position: [
            clean_zero(raw_min_x + offset_x),
            clean_zero(raw_min_y + offset_y),
        ],
        size: [
            clean_zero(raw_max_x - raw_min_x),
            clean_zero(raw_max_y - raw_min_y),
        ],
        ascent,
        descent,
        offset: [clean_zero(offset_x), clean_zero(offset_y)],
        advance: [adv_x, adv_y],
    };

    Some(ShapedLayout {
        metrics,
        glyphs: shaped_glyphs,
    })
}

/// Flattens glyph Bézier outlines into closed 2D polylines in OpenSCAD space.
struct FlattenPen {
    origin: [f64; 2],
    pixel_scale: f64,
    tol_sq: f64,
    /// Fixed parameter steps per curve instead of the tolerance.
    steps: Option<u32>,
    contours: Vec<Vec<[f64; 2]>>,
    current: Vec<[f64; 2]>,
    last: [f64; 2],
}

impl FlattenPen {
    const MAX_DEPTH: u8 = 12;

    fn new(origin: [f64; 2], pixel_scale: f64, mode: Flatten) -> Self {
        let (tol_sq, steps) = match mode {
            Flatten::Tolerance(tol) => (tol * tol, None),
            Flatten::Steps(n) => (0.0, Some(n)),
        };
        Self {
            origin,
            pixel_scale,
            tol_sq,
            steps,
            contours: Vec::new(),
            current: Vec::new(),
            last: origin,
        }
    }

    fn map_pt(&self, x: f32, y: f32) -> [f64; 2] {
        [
            self.origin[0] + f64::from(x) * self.pixel_scale,
            self.origin[1] + f64::from(y) * self.pixel_scale,
        ]
    }

    fn flush_current(&mut self) {
        if self.current.len() >= 3 {
            if self.current.first() == self.current.last() {
                self.current.pop();
            }
            if self.current.len() >= 3 {
                self.contours.push(std::mem::take(&mut self.current));
                return;
            }
        }
        self.current.clear();
    }

    fn finish(mut self) -> Vec<Vec<[f64; 2]>> {
        self.flush_current();
        self.contours
    }

    fn quad_rec(&mut self, p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], depth: u8) {
        let mid_chord = [0.5 * (p0[0] + p2[0]), 0.5 * (p0[1] + p2[1])];
        let mid_curve = [
            0.25 * p0[0] + 0.5 * p1[0] + 0.25 * p2[0],
            0.25 * p0[1] + 0.5 * p1[1] + 0.25 * p2[1],
        ];
        let dx = mid_curve[0] - mid_chord[0];
        let dy = mid_curve[1] - mid_chord[1];
        if depth >= Self::MAX_DEPTH || dx * dx + dy * dy <= self.tol_sq {
            self.current.push(p2);
            return;
        }
        let p01 = [0.5 * (p0[0] + p1[0]), 0.5 * (p0[1] + p1[1])];
        let p12 = [0.5 * (p1[0] + p2[0]), 0.5 * (p1[1] + p2[1])];
        self.quad_rec(p0, p01, mid_curve, depth + 1);
        self.quad_rec(mid_curve, p12, p2, depth + 1);
    }

    fn cubic_rec(&mut self, p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2], depth: u8) {
        let ux = 3.0 * p1[0] - 2.0 * p0[0] - p3[0];
        let uy = 3.0 * p1[1] - 2.0 * p0[1] - p3[1];
        let vx = 3.0 * p2[0] - 2.0 * p3[0] - p0[0];
        let vy = 3.0 * p2[1] - 2.0 * p3[1] - p0[1];
        let err_sq = ux.max(vx) * ux.max(vx) + uy.max(vy) * uy.max(vy);
        if depth >= Self::MAX_DEPTH || err_sq <= 16.0 * self.tol_sq {
            self.current.push(p3);
            return;
        }
        let p01 = [0.5 * (p0[0] + p1[0]), 0.5 * (p0[1] + p1[1])];
        let p12 = [0.5 * (p1[0] + p2[0]), 0.5 * (p1[1] + p2[1])];
        let p23 = [0.5 * (p2[0] + p3[0]), 0.5 * (p2[1] + p3[1])];
        let p012 = [0.5 * (p01[0] + p12[0]), 0.5 * (p01[1] + p12[1])];
        let p123 = [0.5 * (p12[0] + p23[0]), 0.5 * (p12[1] + p23[1])];
        let mid = [0.5 * (p012[0] + p123[0]), 0.5 * (p012[1] + p123[1])];
        self.cubic_rec(p0, p01, p012, mid, depth + 1);
        self.cubic_rec(mid, p123, p23, p3, depth + 1);
    }
}

impl OutlinePen for FlattenPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush_current();
        let pt = self.map_pt(x, y);
        self.current.push(pt);
        self.last = pt;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let pt = self.map_pt(x, y);
        self.current.push(pt);
        self.last = pt;
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let p0 = self.last;
        let p1 = self.map_pt(cx0, cy0);
        let p2 = self.map_pt(x, y);
        if let Some(n) = self.steps {
            for k in 1..=n {
                let t = f64::from(k) / f64::from(n);
                let u = 1.0 - t;
                let (a, b, c) = (u * u, 2.0 * u * t, t * t);
                self.current.push([
                    a * p0[0] + b * p1[0] + c * p2[0],
                    a * p0[1] + b * p1[1] + c * p2[1],
                ]);
            }
        } else {
            self.quad_rec(p0, p1, p2, 0);
        }
        self.last = p2;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let p0 = self.last;
        let p1 = self.map_pt(cx0, cy0);
        let p2 = self.map_pt(cx1, cy1);
        let p3 = self.map_pt(x, y);
        if let Some(n) = self.steps {
            for k in 1..=n {
                let t = f64::from(k) / f64::from(n);
                let u = 1.0 - t;
                let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                self.current.push([
                    a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
                    a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1],
                ]);
            }
        } else {
            self.cubic_rec(p0, p1, p2, p3, 0);
        }
        self.last = p3;
    }

    fn close(&mut self) {
        self.flush_current();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contours_match_metrics_bounds() {
        let params = TextParams {
            text: "OpenSCAD",
            size: 12.0,
            halign: "center",
            valign: "center",
            ..TextParams::default()
        };
        let metrics = measure_text(&params);
        let contours = shape_contours(&params, 0.01);
        assert!(contours.len() >= 8);

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for ring in &contours {
            assert!(ring.len() >= 3);
            for &[x, y] in ring {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        assert!((min_x - metrics.position[0]).abs() < 0.05);
        assert!((min_y - metrics.position[1]).abs() < 0.05);
        assert!(((max_x - min_x) - metrics.size[0]).abs() < 0.05);
        assert!(((max_y - min_y) - metrics.size[1]).abs() < 0.05);
    }
}

#[cfg(test)]
mod step_tests {
    use super::*;

    #[test]
    fn fixed_steps_per_curve() {
        // Liberation Sans "O": two contours of quadratic curves only, 19 in
        // total, so each step count gives 19 * steps points.
        let params = TextParams {
            text: "O",
            ..TextParams::default()
        };
        for steps in [2, 4] {
            let n: usize = shape_contours_steps(&params, steps)
                .iter()
                .map(Vec::len)
                .sum();
            assert_eq!(n, 19 * steps as usize);
        }
    }
}
