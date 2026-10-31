//! The geometry tree produced by evaluating an OpenSCAD program: primitives,
//! transformations and boolean operations with fully resolved parameters.
//! Meshing happens later, from this tree.

use glam::{DMat4, DVec2, DVec3};

/// Where in the source a node was instantiated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceRef {
    pub file: u32,
    pub start: u32,
    pub end: u32,
}

impl SourceRef {
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    pub fn contains(&self, file: u32, offset: u32) -> bool {
        self.file == file && offset >= self.start && offset <= self.end
    }
}

/// Tessellation settings (`$fn`, `$fa`, `$fs`) captured at instantiation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segments {
    pub fn_: f64,
    pub fa: f64,
    pub fs: f64,
}

impl Default for Segments {
    fn default() -> Self {
        Self {
            fn_: 0.0,
            fa: 12.0,
            fs: 2.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OffsetKind {
    Radius(f64),
    Delta { delta: f64, chamfer: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// Groups children without an operation of its own (user modules,
    /// `for`, `if`, implicit unions). `name` is the instantiated module.
    Group {
        name: String,
    },
    Union,
    Difference,
    Intersection,
    Hull,
    Minkowski {
        convexity: f64,
    },
    Render {
        convexity: f64,
    },
    Multmatrix(DMat4),
    Color {
        rgba: [f64; 4],
    },
    Resize {
        size: DVec3,
        auto: [bool; 3],
        convexity: f64,
    },
    LinearExtrude {
        height: f64,
        /// Unit extrusion direction when not +Z.
        direction: Option<DVec3>,
        center: bool,
        convexity: f64,
        twist: f64,
        slices: Option<f64>,
        segments: Option<f64>,
        scale: DVec2,
        tessellation: Segments,
    },
    RotateExtrude {
        angle: f64,
        start: f64,
        convexity: f64,
        tessellation: Segments,
    },
    Fill,
    Offset {
        kind: OffsetKind,
        tessellation: Segments,
    },
    Projection {
        cut: bool,
        convexity: f64,
    },
    Cube {
        size: DVec3,
        center: bool,
    },
    Sphere {
        r: f64,
        /// `style = "octa"` instead of the classic `"orig"` tessellation.
        octa: bool,
        tessellation: Segments,
    },
    Cylinder {
        h: f64,
        r1: f64,
        r2: f64,
        center: bool,
        tessellation: Segments,
    },
    Polyhedron {
        points: Vec<DVec3>,
        faces: Vec<Vec<usize>>,
        convexity: f64,
    },
    Square {
        size: DVec2,
        center: bool,
    },
    Circle {
        r: f64,
        tessellation: Segments,
    },
    Polygon {
        points: Vec<DVec2>,
        paths: Option<Vec<Vec<usize>>>,
        convexity: f64,
    },
    Text {
        text: String,
        size: f64,
        spacing: f64,
        font: String,
        direction: String,
        language: String,
        script: Option<String>,
        halign: String,
        valign: String,
        tessellation: Segments,
    },
    Import {
        file: String,
        layer: Option<String>,
        origin: DVec2,
        dpi: Option<f64>,
        scale: f64,
        center: bool,
        convexity: f64,
        tessellation: Segments,
        timestamp: i64,
    },
    Surface {
        file: String,
        center: bool,
        invert: bool,
        convexity: f64,
        timestamp: i64,
    },
    /// Builtins not meshed yet, kept with their arguments already formatted
    /// for the CSG export.
    Other {
        name: String,
        args: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeModifiers {
    pub root: bool,
    pub highlight: bool,
    pub background: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub children: Vec<Node>,
    pub modifiers: NodeModifiers,
    /// Immediate statement that created this node (may be inside a user module or library).
    pub source: Option<SourceRef>,
    /// Top-level statement in the evaluated program that triggered this subtree.
    pub root_call: Option<SourceRef>,
}

impl Node {
    pub fn new(kind: NodeKind, children: Vec<Node>) -> Self {
        Self {
            kind,
            children,
            modifiers: NodeModifiers::default(),
            source: None,
            root_call: None,
        }
    }

    pub fn group(name: impl Into<String>, children: Vec<Node>) -> Self {
        Self::new(NodeKind::Group { name: name.into() }, children)
    }

    pub fn with_source(mut self, source: SourceRef) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_root_call(mut self, root_call: Option<SourceRef>) -> Self {
        self.root_call = root_call;
        self
    }

    /// Finds the deepest node whose `source` (or `root_call`) covers `(file, offset)`.
    /// Useful for highlighting 3D geometry at the editor cursor position.
    pub fn find_at_offset(&self, file: u32, offset: u32) -> Option<&Node> {
        for child in &self.children {
            if let Some(found) = child.find_at_offset(file, offset) {
                return Some(found);
            }
        }
        let matches_self = self.source.is_some_and(|s| s.contains(file, offset))
            || self.root_call.is_some_and(|s| s.contains(file, offset));
        matches_self.then_some(self)
    }
}
