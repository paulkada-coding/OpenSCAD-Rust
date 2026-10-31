//! Turns the evaluated CSG tree ([`geometry::csg::Node`]) into meshes using
//! the Manifold kernel: OpenSCAD-exact tessellation of primitives, then
//! transformations, booleans, extrusions and 2D operations.
//!
//! This is the contract shared by the app, the `scadr` CLI and the tests.
//! The implementation is still a stub that returns empty geometry.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use geometry::Mesh;
use geometry::csg::{Node, SourceRef};
use geometry::glam::DVec2;

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
    /// Source locations referenced by `triangle_sources`.
    pub sources: Vec<SourceRef>,
    /// OpenSCAD-style warnings raised while meshing (without the
    /// `WARNING:` prefix).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Checked between operations; when set, rendering stops with
    /// [`RenderError::Cancelled`].
    pub cancel: Option<Arc<AtomicBool>>,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("calcolo annullato")]
    Cancelled,
    #[error("errore del kernel geometrico: {0}")]
    Kernel(String),
}

/// Renders the top-level nodes returned by the evaluator (their implicit
/// union), honoring the `!`, `#`, `%` modifiers like OpenSCAD.
pub fn render(nodes: &[Node], options: &RenderOptions) -> Result<RenderOutput, RenderError> {
    let _ = (nodes, options);
    Ok(RenderOutput::default())
}
