//! Walks the CSG tree and combines child geometry like OpenSCAD's
//! geometry evaluator.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use geometry::csg::{Node, NodeKind, OffsetKind, SourceRef};
use geometry::glam::{DAffine3, DMat4, DVec2, DVec3};
use manifold::clipper::Join;
use manifold::{FillRule, Manifold, OpType};

use crate::extrude::{self, LinearParams, RotateParams};
use crate::fragments::{cos_degrees, fragments};
use crate::planar::{self, Planar};
use crate::polyset::PolySet;
use crate::{Diagnostic, Level, RenderError, RenderOptions, primitives};

/// Geometry of a subtree.
#[derive(Debug, Clone, Default)]
pub enum Geom {
    #[default]
    Empty,
    /// Exact primitive tessellation, not yet in the kernel; `source` is an
    /// index into [`Evaluator::sources`].
    Poly {
        ps: PolySet,
        source: Option<u32>,
    },
    Solid(Manifold),
    Planar(Planar),
}

impl Geom {
    fn dim(&self) -> Option<u8> {
        match self {
            Self::Empty => None,
            Self::Poly { .. } | Self::Solid(_) => Some(3),
            Self::Planar(_) => Some(2),
        }
    }

    pub fn transform(self, m: &DMat4) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Poly { mut ps, source } => {
                ps.transform(m);
                Self::Poly { ps, source }
            }
            Self::Solid(s) => {
                if !m.is_finite() || m.determinant() == 0.0 {
                    return Self::Empty;
                }
                Self::Solid(s.transform(&DAffine3::from_mat4(*m)))
            }
            Self::Planar(mut p) => {
                p.transform(m);
                if p.is_empty() {
                    Self::Empty
                } else {
                    Self::Planar(p)
                }
            }
        }
    }
}

pub struct Evaluator<'a> {
    options: &'a RenderOptions,
    pub diagnostics: Vec<Diagnostic>,
    pub sources: Vec<SourceRef>,
    source_index: HashMap<SourceRef, u32>,
    /// Kernel original ID -> index into `sources`.
    pub original_sources: HashMap<u32, u32>,
    /// World transform of the node being evaluated.
    world: DMat4,
    /// `#` subtrees, already in world coordinates.
    pub highlighted: Vec<Geom>,
    /// `%` subtrees with their world transform, rendered at the end.
    pub background: Vec<(Node, DMat4)>,
    /// Inside a `%` subtree, `#` is not collected again.
    in_background: bool,
}

type Result<T> = std::result::Result<T, RenderError>;

impl<'a> Evaluator<'a> {
    pub fn new(options: &'a RenderOptions) -> Self {
        Self {
            options,
            diagnostics: Vec::new(),
            sources: Vec::new(),
            source_index: HashMap::new(),
            original_sources: HashMap::new(),
            world: DMat4::IDENTITY,
            highlighted: Vec::new(),
            background: Vec::new(),
            in_background: false,
        }
    }

    /// Evaluates a subtree marked `%` for display only.
    pub fn eval_background(&mut self, node: &Node, world: DMat4) -> Result<Geom> {
        let saved = (self.world, self.in_background);
        self.world = world;
        self.in_background = true;
        let g = self.eval(node);
        (self.world, self.in_background) = saved;
        Ok(g?.transform(&world))
    }

    pub fn options(&self) -> &RenderOptions {
        self.options
    }

    pub fn warn(&mut self, text: impl Into<String>, source: Option<SourceRef>) {
        self.diagnostics.push(Diagnostic {
            level: Level::Warning,
            text: text.into(),
            source,
        });
    }

    pub fn error(&mut self, text: impl Into<String>, source: Option<SourceRef>) {
        self.diagnostics.push(Diagnostic {
            level: Level::Error,
            text: text.into(),
            source,
        });
    }

    fn check_cancel(&self) -> Result<()> {
        match &self.options.cancel {
            Some(flag) if flag.load(Ordering::Relaxed) => Err(RenderError::Cancelled),
            _ => Ok(()),
        }
    }

    fn source_id(&mut self, source: Option<SourceRef>) -> Option<u32> {
        let source = source?;
        Some(*self.source_index.entry(source).or_insert_with(|| {
            self.sources.push(source);
            (self.sources.len() - 1) as u32
        }))
    }

    /// Tags a kernel solid built for `node` as an original mesh, so that
    /// its triangles can be traced back to the node.
    fn tag(&mut self, m: Manifold, source: Option<SourceRef>) -> Manifold {
        let Some(id) = self.source_id(source) else {
            return m;
        };
        let m = m.as_original();
        let original = m.original_id();
        if original >= 0 {
            self.original_sources.insert(original as u32, id);
        }
        m
    }

    /// Kernel solid of a 3D geometry; `None` for empty or failed input.
    pub fn to_solid(&mut self, g: Geom) -> Option<Manifold> {
        match g {
            Geom::Solid(m) => Some(m),
            Geom::Poly { ps, source } => {
                let mesh = ps.triangulate();
                let m = match Manifold::from_mesh(&mesh) {
                    Ok(m) => m,
                    Err(err) => {
                        self.warn(
                            format!("PolySet -> Manifold conversion failed: {}", error_name(err)),
                            None,
                        );
                        match Manifold::from_mesh_merged(&mesh) {
                            Ok(m) => m,
                            Err(err) => {
                                self.error("[manifold] Input mesh is not closed!", None);
                                self.error(
                                    format!(
                                        "[manifold] Surface_mesh -> Manifold conversion failed: {}",
                                        error_name(err)
                                    ),
                                    None,
                                );
                                return None;
                            }
                        }
                    }
                };
                let original = m.original_id();
                if original >= 0
                    && let Some(id) = source
                {
                    self.original_sources.insert(original as u32, id);
                }
                Some(m)
            }
            Geom::Empty | Geom::Planar(_) => None,
        }
    }

    pub fn eval(&mut self, node: &Node) -> Result<Geom> {
        self.check_cancel()?;
        let src = node.source;
        Ok(match &node.kind {
            NodeKind::Group { .. } | NodeKind::Union | NodeKind::Color { .. } => {
                let children = self.eval_children(&node.children)?;
                self.union(children, src)
            }
            NodeKind::Render { .. } => {
                let children = self.eval_children(&node.children)?;
                match self.union(children, src) {
                    g @ Geom::Poly { .. } => match self.to_solid(g) {
                        Some(m) => Geom::Solid(m),
                        None => Geom::Empty,
                    },
                    g => g,
                }
            }
            NodeKind::Difference => {
                let children = self.eval_children(&node.children)?;
                self.difference(children, src)
            }
            NodeKind::Intersection => {
                let children = self.eval_children(&node.children)?;
                self.intersection(children, src)
            }
            NodeKind::Multmatrix(m) => {
                let saved = self.world;
                self.world = saved * *m;
                let children = self.eval_children(&node.children);
                self.world = saved;
                let g = self.union(children?, src);
                g.transform(m)
            }
            NodeKind::Hull => {
                let children = self.eval_children(&node.children)?;
                self.hull(children, src)
            }
            NodeKind::Minkowski { .. } => {
                let children = self.eval_children(&node.children)?;
                self.minkowski(children, src)?
            }
            NodeKind::Resize { size, auto, .. } => {
                let children = self.eval_children(&node.children)?;
                let g = self.union(children, src);
                self.resize(g, *size, *auto)
            }
            NodeKind::Fill => match self.planar_children(node)? {
                Some(p) => planar_geom(p.without_holes()),
                None => Geom::Empty,
            },
            NodeKind::Offset { kind, tessellation } => match self.planar_children(node)? {
                Some(p) => {
                    let out = match kind {
                        OffsetKind::Radius(r) => {
                            if *r == 0.0 {
                                p
                            } else {
                                let n = fragments(r.abs(), tessellation);
                                let tol = r.abs() * (1.0 - cos_degrees(180.0 / f64::from(n)));
                                planar::offset(&p, *r, Join::Round, tol)
                            }
                        }
                        OffsetKind::Delta { delta, chamfer } => {
                            let join = if *chamfer { Join::Square } else { Join::Miter };
                            planar::offset(&p, *delta, join, 0.0)
                        }
                    };
                    planar_geom(out)
                }
                None => Geom::Empty,
            },
            NodeKind::Projection { cut, .. } => {
                let children = self.eval_children(&node.children)?;
                let g = self.union_dim(children, src, Some(3));
                match self.to_solid(g) {
                    Some(m) => {
                        let cs = if *cut { m.slice(0.0) } else { m.project() };
                        planar_geom(Planar::from_contours(&cs.to_polygons(), FillRule::Positive))
                    }
                    None => Geom::Empty,
                }
            }
            NodeKind::LinearExtrude {
                height,
                direction,
                center,
                twist,
                slices,
                segments,
                scale,
                tessellation,
                ..
            } => match self.planar_children(node)? {
                Some(p) => {
                    let params = LinearParams {
                        height: *height,
                        direction: *direction,
                        center: *center,
                        twist: *twist,
                        slices: *slices,
                        segments: *segments,
                        scale: *scale,
                        tessellation: *tessellation,
                    };
                    self.poly(extrude::linear_extrude(&p, &params), src)
                }
                None => Geom::Empty,
            },
            NodeKind::RotateExtrude {
                angle,
                start,
                tessellation,
                ..
            } => match self.planar_children(node)? {
                Some(p) => {
                    let params = RotateParams {
                        angle: *angle,
                        start: *start,
                        tessellation: *tessellation,
                    };
                    match extrude::rotate_extrude(&p, &params) {
                        Ok(ps) => self.poly(ps, src),
                        Err(e) => {
                            self.error(e.message(), src);
                            Geom::Empty
                        }
                    }
                }
                None => Geom::Empty,
            },
            NodeKind::Cube { size, center } => self.poly(primitives::cube(*size, *center), src),
            NodeKind::Sphere {
                r,
                octa,
                tessellation,
            } => self.poly(primitives::sphere(*r, *octa, tessellation), src),
            NodeKind::Cylinder {
                h,
                r1,
                r2,
                center,
                tessellation,
            } => self.poly(
                primitives::cylinder(*h, *r1, *r2, *center, tessellation),
                src,
            ),
            NodeKind::Polyhedron { points, faces, .. } => {
                let r = primitives::polyhedron(points, faces);
                for w in r.warnings {
                    self.warn(w, src);
                }
                self.poly(Some(r.polyset), src)
            }
            NodeKind::Square { size, center } => {
                primitives::square(*size, *center).map_or(Geom::Empty, Geom::Planar)
            }
            NodeKind::Circle { r, tessellation } => {
                primitives::circle(*r, tessellation).map_or(Geom::Empty, Geom::Planar)
            }
            NodeKind::Polygon { points, paths, .. } => {
                planar_geom(primitives::polygon(points, paths.as_deref()))
            }
            NodeKind::Text { .. } => crate::text::text(self, node),
            NodeKind::Import { .. } => crate::import::import(self, node),
            NodeKind::Surface { .. } => crate::import::surface(self, node),
            NodeKind::Other { name, .. } => {
                self.warn(format!("{name}() is not supported yet, ignored"), src);
                Geom::Empty
            }
        })
    }

    /// Wraps a primitive mesh, remembering where it came from.
    pub fn poly(&mut self, ps: Option<PolySet>, source: Option<SourceRef>) -> Geom {
        match ps {
            Some(ps) if !ps.is_empty() => Geom::Poly {
                ps,
                source: self.source_id(source),
            },
            _ => Geom::Empty,
        }
    }

    /// Evaluates the children of `parent`, setting aside `%` subtrees and
    /// collecting `#` ones.
    /// The implicit union of the program's top-level nodes.
    pub fn eval_top(&mut self, nodes: &[Node]) -> Result<Geom> {
        let children = self.eval_children(nodes)?;
        Ok(self.union(children, None))
    }

    fn eval_children(&mut self, children: &[Node]) -> Result<Vec<Geom>> {
        let mut out = Vec::with_capacity(children.len());
        for child in children {
            if child.modifiers.background {
                if !self.in_background {
                    self.background.push((child.clone(), self.world));
                }
                continue;
            }
            let g = self.eval(child)?;
            if child.modifiers.highlight && !self.in_background && !matches!(g, Geom::Empty) {
                self.highlighted.push(g.clone().transform(&self.world));
            }
            out.push(g);
        }
        Ok(out)
    }

    /// 2D union of the children of a 2D operation (3D children are ignored
    /// with OpenSCAD's warnings).
    fn planar_children(&mut self, node: &Node) -> Result<Option<Planar>> {
        let children = self.eval_children(&node.children)?;
        Ok(match self.union_dim(children, node.source, Some(2)) {
            Geom::Planar(p) => Some(p),
            _ => None,
        })
    }

    /// Drops children whose dimension differs from the operation's (the
    /// first non-empty child, or `want`), with OpenSCAD's warnings.
    fn filter_dim(
        &mut self,
        children: Vec<Geom>,
        source: Option<SourceRef>,
        want: Option<u8>,
    ) -> Vec<Geom> {
        let children: Vec<Geom> = children.into_iter().filter(|g| g.dim().is_some()).collect();
        let Some(dim) = want.or_else(|| children.first().and_then(Geom::dim)) else {
            return children;
        };
        let mut mixed = false;
        let mut out = Vec::with_capacity(children.len());
        for g in children {
            if g.dim() == Some(dim) {
                out.push(g);
                continue;
            }
            if want.is_none() && !mixed {
                mixed = true;
                self.warn("Mixing 2D and 3D objects is not supported", source);
            }
            if dim == 3 {
                self.warn("Ignoring 2D child object for 3D operation", source);
            } else {
                self.warn("Ignoring 3D child object for 2D operation", source);
            }
        }
        out
    }

    pub fn union(&mut self, children: Vec<Geom>, source: Option<SourceRef>) -> Geom {
        self.union_dim(children, source, None)
    }

    fn union_dim(
        &mut self,
        children: Vec<Geom>,
        source: Option<SourceRef>,
        want: Option<u8>,
    ) -> Geom {
        let mut children = self.filter_dim(children, source, want);
        match children.len() {
            0 => Geom::Empty,
            1 => children.pop().unwrap_or_default(),
            _ => {
                if children[0].dim() == Some(2) {
                    let items: Vec<Planar> = children.into_iter().filter_map(into_planar).collect();
                    planar_geom(planar::union(&items))
                } else {
                    let solids: Vec<Manifold> = children
                        .into_iter()
                        .filter_map(|g| self.to_solid(g))
                        .collect();
                    solid_geom(Manifold::batch_union(&solids))
                }
            }
        }
    }

    fn difference(&mut self, children: Vec<Geom>, source: Option<SourceRef>) -> Geom {
        // The first child decides; an empty first child empties the result.
        if children.first().is_none_or(|g| g.dim().is_none()) {
            return Geom::Empty;
        }
        let mut children = self.filter_dim(children, source, None);
        if children.len() == 1 {
            return children.pop().unwrap_or_default();
        }
        if children[0].dim() == Some(2) {
            let mut items: Vec<Planar> = children.into_iter().filter_map(into_planar).collect();
            let first = items.remove(0);
            planar_geom(planar::difference(&first, &items))
        } else {
            let mut iter = children.into_iter();
            let Some(first) = iter.next().and_then(|g| self.to_solid(g)) else {
                return Geom::Empty;
            };
            let mut solids = vec![first];
            solids.extend(iter.filter_map(|g| self.to_solid(g)));
            solid_geom(Manifold::batch_boolean(&solids, OpType::Subtract))
        }
    }

    fn intersection(&mut self, children: Vec<Geom>, source: Option<SourceRef>) -> Geom {
        if children.iter().any(|g| g.dim().is_none()) {
            return Geom::Empty;
        }
        let mut children = self.filter_dim(children, source, None);
        match children.len() {
            0 => Geom::Empty,
            1 => children.pop().unwrap_or_default(),
            _ if children[0].dim() == Some(2) => {
                let items: Vec<Planar> = children.into_iter().filter_map(into_planar).collect();
                planar_geom(planar::intersection(&items))
            }
            _ => {
                let solids: Vec<Manifold> = children
                    .into_iter()
                    .filter_map(|g| self.to_solid(g))
                    .collect();
                solid_geom(Manifold::batch_boolean(&solids, OpType::Intersect))
            }
        }
    }

    fn hull(&mut self, children: Vec<Geom>, source: Option<SourceRef>) -> Geom {
        let children = self.filter_dim(children, source, None);
        match children.first().and_then(Geom::dim) {
            None => Geom::Empty,
            Some(2) => {
                let points: Vec<DVec2> = children
                    .iter()
                    .filter_map(|g| match g {
                        Geom::Planar(p) => Some(p.contours.iter().flatten().copied()),
                        _ => None,
                    })
                    .flatten()
                    .collect();
                let hull = convex_hull_2d(&points);
                if hull.len() < 3 {
                    Geom::Empty
                } else {
                    Geom::Planar(Planar::new(vec![hull]))
                }
            }
            Some(_) => {
                let mut points = Vec::new();
                for g in children {
                    match g {
                        Geom::Poly { ps, .. } => points.extend(ps.vertices),
                        Geom::Solid(m) => points.extend(m.to_mesh().vertices),
                        _ => {}
                    }
                }
                let hull = Manifold::hull_of_points(&points);
                if hull.is_empty() || hull.volume() <= 0.0 {
                    Geom::Empty
                } else {
                    Geom::Solid(self.tag(hull, source))
                }
            }
        }
    }

    fn minkowski(&mut self, children: Vec<Geom>, source: Option<SourceRef>) -> Result<Geom> {
        let mut children = self.filter_dim(children, source, None);
        if children.len() <= 1 {
            return Ok(children.pop().unwrap_or_default());
        }
        if children[0].dim() == Some(2) {
            let mut items = children.into_iter().filter_map(into_planar);
            let mut acc = items.next().unwrap_or_default();
            for item in items {
                self.check_cancel()?;
                acc = minkowski_2d(&acc, &item);
            }
            return Ok(planar_geom(acc));
        }
        let solids: Vec<Manifold> = children
            .into_iter()
            .filter_map(|g| self.to_solid(g))
            .collect();
        let mut solids = solids.into_iter();
        let Some(mut acc) = solids.next() else {
            return Ok(Geom::Empty);
        };
        for s in solids {
            self.check_cancel()?;
            acc = acc.minkowski_sum(&s);
        }
        Ok(match solid_geom(acc) {
            Geom::Solid(m) => Geom::Solid(self.tag(m, source)),
            g => g,
        })
    }

    fn resize(&mut self, g: Geom, size: DVec3, auto: [bool; 3]) -> Geom {
        let (min, max, dims) = match &g {
            Geom::Empty => return g,
            Geom::Poly { ps, .. } => {
                let b = ps.triangulate().bounds();
                (b.min, b.max, 3)
            }
            Geom::Solid(m) => {
                let b = m.bounds();
                (b.min, b.max, 3)
            }
            Geom::Planar(p) => {
                let (lo, hi) = p.bounds();
                (lo.extend(0.0), hi.extend(0.0), 2)
            }
        };
        let current = max - min;
        let mut scale = DVec3::ONE;
        for i in 0..dims {
            if size[i] > 0.0 && current[i] > 0.0 {
                scale[i] = size[i] / current[i];
            }
        }
        // Auto axes take the largest of the explicit factors.
        let explicit = (0..dims)
            .filter(|&i| size[i] > 0.0 && current[i] > 0.0)
            .map(|i| scale[i])
            .fold(f64::NAN, f64::max);
        if !explicit.is_nan() {
            for i in 0..dims {
                if auto[i] && size[i] <= 0.0 {
                    scale[i] = explicit;
                }
            }
        }
        g.transform(&DMat4::from_scale(scale))
    }
}

pub fn error_name(err: manifold::Error) -> &'static str {
    use manifold::Error as E;
    match err {
        E::NonFiniteVertex => "NonFiniteVertex",
        E::NotManifold => "NotManifold",
        E::VertexIndexOutOfBounds => "VertexOutOfBounds",
        E::PropertiesWrongLength => "PropertiesWrongLength",
        E::MissingPositionProperties => "MissingPositionProperties",
        E::MergeVectorsDifferentLengths => "MergeVectorsDifferentLengths",
        E::MergeIndexOutOfBounds => "MergeIndexOutOfBounds",
        E::TransformWrongLength => "TransformWrongLength",
        E::RunIndexWrongLength => "RunIndexWrongLength",
        E::FaceIdWrongLength => "FaceIDWrongLength",
        E::InvalidConstruction => "InvalidConstruction",
        E::ResultTooLarge => "ResultTooLarge",
        E::InvalidTangents => "InvalidTangents",
        E::Cancelled => "Cancelled",
        E::Unknown(_) => "Unknown",
    }
}

fn into_planar(g: Geom) -> Option<Planar> {
    match g {
        Geom::Planar(p) => Some(p),
        _ => None,
    }
}

pub fn planar_geom(p: Planar) -> Geom {
    if p.is_empty() {
        Geom::Empty
    } else {
        Geom::Planar(p)
    }
}

fn solid_geom(m: Manifold) -> Geom {
    if m.is_empty() {
        Geom::Empty
    } else {
        Geom::Solid(m)
    }
}

/// Convex hull (Andrew's monotone chain), counter-clockwise, keeping the
/// input coordinates exactly.
pub fn convex_hull_2d(points: &[DVec2]) -> Vec<DVec2> {
    let mut pts: Vec<DVec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<DVec2> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &DVec2>> = if pass == 0 {
            Box::new(pts.iter())
        } else {
            Box::new(pts.iter().rev())
        };
        for &p in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// Minkowski sum of two regions: the outlines swept along each other,
/// plus each region translated by a point of the other (fills interiors).
fn minkowski_2d(a: &Planar, b: &Planar) -> Planar {
    use manifold::clipper::{self, ClipOp};
    let pa = a.to_paths();
    let pb = b.to_paths();
    let (Some(a0), Some(b0)) = (
        pa.first().and_then(|c| c.first()),
        pb.first().and_then(|c| c.first()),
    ) else {
        return Planar::default();
    };
    let mut parts: Vec<Vec<[i64; 2]>> = Vec::new();
    for ca in &pa {
        for cb in &pb {
            parts.extend(clipper::minkowski_sum(cb, ca));
        }
    }
    parts.extend(
        pa.iter()
            .map(|c| c.iter().map(|p| [p[0] + b0[0], p[1] + b0[1]]).collect()),
    );
    parts.extend(
        pb.iter()
            .map(|c| c.iter().map(|p| [p[0] + a0[0], p[1] + a0[1]]).collect()),
    );
    planar::from_paths(&clipper::boolean(
        &parts,
        &[],
        ClipOp::Union,
        FillRule::NonZero,
    ))
}
