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
    let _ = ev;
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
    // OpenSCAD splits every curve into the same number of steps (fitted
    // to its output): max(2, floor(fragments(size) / 8) + 1).
    let steps = (crate::fragments::fragments(size.abs(), tessellation) / 8 + 1).max(2);
    let contours: Vec<Vec<DVec2>> = scad_font::shape_contours_steps(&params, steps)
        .into_iter()
        .map(|c| c.into_iter().map(DVec2::from_array).collect())
        .collect();
    planar_geom(Planar::from_contours(&contours, FillRule::NonZero))
}
