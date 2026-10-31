//! `text()` from the shaped glyph outlines of `scad-font`.

use geometry::csg::{Node, NodeKind};
use geometry::glam::DVec2;
use manifold::FillRule;

use crate::eval::{Evaluator, Geom, planar_geom};
use crate::planar::Planar;

pub fn text(ev: &mut Evaluator, node: &Node) -> Geom {
    let NodeKind::Text {
        text,
        size,
        spacing,
        font,
        direction,
        language,
        script,
        halign,
        valign,
        tessellation,
    } = &node.kind
    else {
        return Geom::Empty;
    };
    let _ = (ev, tessellation);
    let params = scad_font::TextParams {
        text,
        size: *size,
        font,
        halign,
        valign,
        spacing: *spacing,
        direction,
        language,
        script: script.as_deref().unwrap_or(""),
    };
    let tolerance = size.abs() * 1e-3;
    let contours: Vec<Vec<DVec2>> = scad_font::shape_contours(&params, tolerance)
        .into_iter()
        .map(|c| c.into_iter().map(DVec2::from_array).collect())
        .collect();
    planar_geom(Planar::from_contours(&contours, FillRule::NonZero))
}
