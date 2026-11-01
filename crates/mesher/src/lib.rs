//! Turns the evaluated CSG tree ([`geometry::csg::Node`]) into meshes using
//! the Manifold kernel: OpenSCAD-exact tessellation of primitives, then
//! transformations, booleans, extrusions and 2D operations.
//!
//! This is the contract shared by the app, the `scadr` CLI and the tests.
//!
//! How it follows OpenSCAD (observed with OpenSCAD Nightly, Manifold
//! backend):
//! - Primitives are tessellated exactly like OpenSCAD (same vertex
//!   coordinates, bit for bit) and stay plain polygon meshes until an
//!   operation needs the kernel, so a lone primitive (also transformed) is
//!   output unchanged.
//! - 2D operations run Clipper on a fixed 2^-27 grid, like OpenSCAD.
//! - Mixed 2D/3D children, invalid polyhedra and failed conversions produce
//!   OpenSCAD's warning texts.

mod eval;
mod extrude;
mod fragments;
mod import;
mod planar;
mod polyset;
mod primitives;
mod tess;
mod text;

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use geometry::Mesh;
use geometry::csg::{Node, SourceRef};
use geometry::glam::DVec2;

use crate::eval::{Evaluator, Geom};

pub use fragments::fragments;

/// Result geometry of a subtree.
#[derive(Debug, Clone, Default)]
pub enum Shape {
    #[default]
    Empty,
    /// A closed 3D solid.
    Solid(Mesh),
    /// A 2D result (top-level 2D objects).
    Planar {
        /// Closed outlines: outer boundaries counter-clockwise, holes clockwise.
        outlines: Vec<Vec<DVec2>>,
        /// Triangulation of the outlines at z = 0, for display only.
        fill: Mesh,
    },
}

impl Shape {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Solid(mesh) => mesh.is_empty(),
            Self::Planar { outlines, .. } => outlines.is_empty(),
        }
    }
}

/// Severity of a [`Diagnostic`], like OpenSCAD's `WARNING:` / `ERROR:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Warning,
    Error,
}

/// A message raised while meshing, with OpenSCAD's text (no prefix) and
/// the node it is about, when known.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub level: Level,
    pub text: String,
    pub source: Option<SourceRef>,
}

/// Everything produced by rendering a program's top-level nodes.
#[derive(Debug, Clone, Default)]
pub struct RenderOutput {
    /// The final geometry: what an STL export contains.
    pub shape: Shape,
    /// Subtrees marked `#`: part of `shape`, also shown highlighted.
    pub highlighted: Vec<Shape>,
    /// Subtrees marked `%`: shown transparent, excluded from `shape`.
    pub background: Vec<Shape>,
    /// For each triangle of `shape` (when solid), the index into `sources`
    /// of the primitive it comes from, or `u32::MAX` when unknown. May be
    /// empty when source tracking is unavailable.
    pub triangle_sources: Vec<u32>,
    /// Source locations referenced by `triangle_sources`. The same location
    /// can appear more than once, reached through different module calls.
    pub sources: Vec<SourceRef>,
    /// Parallel to `sources`: the chain of nested instantiations that
    /// produced each one (e.g. a module call, a `translate()` inside it,
    /// then the primitive), outermost first, ending with the source itself.
    pub source_paths: Vec<Vec<SourceRef>>,
    /// OpenSCAD-style warnings raised while meshing (without the
    /// `WARNING:` prefix).
    pub warnings: Vec<String>,
    /// Parallel to `warnings`: the node that triggered each warning (the
    /// location OpenSCAD appends as ` in file …, line N`), when known.
    pub warning_sources: Vec<Option<SourceRef>>,
    /// All messages (warnings and OpenSCAD `ERROR:`s that do not stop the
    /// render), in order, with their source nodes.
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Checked between operations; when set, rendering stops with
    /// [`RenderError::Cancelled`].
    pub cancel: Option<Arc<AtomicBool>>,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("evaluation cancelled")]
    Cancelled,
    #[error("geometry kernel error: {0}")]
    Kernel(String),
}

/// Renders the top-level nodes returned by the evaluator (their implicit
/// union), honoring the `!`, `#`, `%` modifiers like OpenSCAD.
pub fn render(nodes: &[Node], options: &RenderOptions) -> Result<RenderOutput, RenderError> {
    render_with_files(nodes, options, &[])
}

/// Like [`render`], with the paths of the source files indexed by
/// [`SourceRef::file`] (as listed by the evaluator): `import()` and
/// `surface()` then resolve relative paths against the directory of the
/// file containing the call, like OpenSCAD. Without an entry they resolve
/// against the current directory.
pub fn render_with_files(
    nodes: &[Node],
    options: &RenderOptions,
    files: &[PathBuf],
) -> Result<RenderOutput, RenderError> {
    // The kernel reports C++ exceptions (e.g. out of memory) as panics.
    match std::panic::catch_unwind(AssertUnwindSafe(|| render_inner(nodes, options, files))) {
        Ok(result) => result,
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_else(|| "internal error".into());
            Err(RenderError::Kernel(message))
        }
    }
}

/// First subtree marked `!` (depth first), like OpenSCAD.
fn find_root(nodes: &[Node]) -> Option<&Node> {
    nodes.iter().find_map(|node| {
        if node.modifiers.root {
            Some(node)
        } else {
            find_root(&node.children)
        }
    })
}

fn render_inner(
    nodes: &[Node],
    options: &RenderOptions,
    files: &[PathBuf],
) -> Result<RenderOutput, RenderError> {
    let mut ev = Evaluator::new(options, files);
    let geom = match find_root(nodes) {
        Some(root) => ev.eval(root)?,
        None => ev.eval_top(nodes)?,
    };
    let mut out = RenderOutput::default();
    let (shape, sources) = to_shape(&mut ev, geom, true)?;
    out.shape = shape;
    out.triangle_sources = sources;
    out.highlighted = std::mem::take(&mut ev.highlighted)
        .into_iter()
        .map(|g| to_shape(&mut ev, g, false).map(|(s, _)| s))
        .collect::<Result<_, _>>()?;
    for (node, world) in std::mem::take(&mut ev.background) {
        let g = ev.eval_background(&node, world)?;
        let (shape, _) = to_shape(&mut ev, g, false)?;
        if !shape.is_empty() {
            out.background.push(shape);
        }
    }
    out.sources = std::mem::take(&mut ev.sources);
    out.source_paths = std::mem::take(&mut ev.source_paths);
    (out.warnings, out.warning_sources) = ev
        .diagnostics
        .iter()
        .filter(|d| d.level == Level::Warning)
        .map(|d| (d.text.clone(), d.source))
        .unzip();
    out.diagnostics = std::mem::take(&mut ev.diagnostics);
    Ok(out)
}

/// Final mesh of a geometry, with per-triangle source indices when
/// `track` is set.
fn to_shape(ev: &mut Evaluator, g: Geom, track: bool) -> Result<(Shape, Vec<u32>), RenderError> {
    Ok(match g {
        Geom::Empty => (Shape::Empty, Vec::new()),
        Geom::Poly { ps, source } => {
            let mesh = ps.triangulate();
            let sources = if track {
                vec![source.unwrap_or(u32::MAX); mesh.triangles.len()]
            } else {
                Vec::new()
            };
            (Shape::Solid(mesh), sources)
        }
        Geom::Solid(m) => {
            if let Err(err) = m.status() {
                ev.error(
                    format!("[manifold] operation failed: {}", eval::error_name(err)),
                    None,
                );
                return Ok((Shape::Empty, Vec::new()));
            }
            let (mesh, runs) = m.to_mesh_with_runs();
            let mut sources = Vec::new();
            if track {
                sources = vec![u32::MAX; mesh.triangles.len()];
                for run in runs {
                    if let Some(&id) = ev.original_sources.get(&run.original_id) {
                        sources[run.triangles].fill(id);
                    }
                }
            }
            if mesh.is_empty() {
                (Shape::Empty, Vec::new())
            } else {
                (Shape::Solid(mesh), sources)
            }
        }
        Geom::Planar(p) => {
            let fill = p.fill_mesh();
            (
                Shape::Planar {
                    outlines: p.contours,
                    fill,
                },
                Vec::new(),
            )
        }
    })
}
