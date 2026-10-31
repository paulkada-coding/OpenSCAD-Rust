//! Geometric metrics of a triangle mesh read from an STL file, used by the
//! geometry mode to compare our engine with OpenSCAD. Everything is
//! independent of the triangle order in the file (triangles are put in a
//! canonical order first, so sums are reproducible to the last bit).

use std::collections::HashMap;
use std::fmt::Write as _;

use geometry::glam::DVec3;

use crate::json::Json;

/// Unique vertex lists are stored in full up to this many vertices.
pub const VERTEX_LIST_LIMIT: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    /// Facets in the file (degenerate ones included).
    pub triangles: usize,
    /// Distinct vertices after welding bit-identical coordinates.
    pub vertices: usize,
    pub volume: f64,
    pub area: f64,
    pub bbox_min: [f64; 3],
    pub bbox_max: [f64; 3],
    /// Every edge shared by exactly two triangles with opposite orientation.
    pub manifold: bool,
    /// Edges used by a single triangle.
    pub open_edges: usize,
    /// Edges used by more than two triangles, or twice in the same direction.
    pub nonmanifold_edges: usize,
    /// Triangles with two welded vertices equal (ignored by the topology).
    pub degenerate_triangles: usize,
    /// Connected pieces (vertices linked by edges).
    pub components: usize,
    /// χ = V - E + F over welded vertices and non-degenerate triangles.
    pub euler: i64,
    /// Manifold's convention, as printed by OpenSCAD: `1 - χ / 2`
    /// (two disjoint cubes give -1).
    pub genus: i64,
    /// FNV-1a 64 of the sorted distinct vertices rounded to 1e-6.
    pub vertex_hash: String,
    /// FNV-1a 64 of the sorted triangles (rounded vertices, canonical rotation).
    pub triangle_hash: String,
    /// The sorted distinct rounded vertices, for small meshes.
    pub vertex_list: Option<Vec<[f64; 3]>>,
}

/// Rounds to 1e-6 in decimal: the canonical text of a coordinate.
fn round_text(v: f64) -> String {
    let s = format!("{v:.6}");
    if s.trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        "0.000000".into()
    } else {
        s
    }
}

fn cmp3(a: &[f64; 3], b: &[f64; 3]) -> std::cmp::Ordering {
    a[0].total_cmp(&b[0])
        .then(a[1].total_cmp(&b[1]))
        .then(a[2].total_cmp(&b[2]))
}

/// Rotates a triangle (keeping its orientation) so the smallest vertex is first.
fn canonical_rotation<T: Copy>(t: [T; 3], less: impl Fn(&T, &T) -> bool) -> [T; 3] {
    let mut k = 0;
    for i in 1..3 {
        if less(&t[i], &t[k]) {
            k = i;
        }
    }
    [t[k], t[(k + 1) % 3], t[(k + 2) % 3]]
}

fn fnv1a64(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{h:016x}")
}

impl Metrics {
    pub fn from_soup(soup: &[[DVec3; 3]]) -> Self {
        // Canonical triangle order: -0 → 0, rotate, sort.
        let mut tris: Vec<[[f64; 3]; 3]> = soup
            .iter()
            .map(|t| {
                let t = t.map(|p| {
                    let p = p + DVec3::ZERO;
                    [p.x, p.y, p.z]
                });
                canonical_rotation(t, |a, b| cmp3(a, b).is_lt())
            })
            .collect();
        tris.sort_by(|a, b| {
            cmp3(&a[0], &b[0])
                .then(cmp3(&a[1], &b[1]))
                .then(cmp3(&a[2], &b[2]))
        });

        // Weld bit-identical vertices.
        let mut index: HashMap<[u64; 3], u32> = HashMap::new();
        let mut verts: Vec<[f64; 3]> = Vec::new();
        let ids: Vec<[u32; 3]> = tris
            .iter()
            .map(|t| {
                t.map(|p| {
                    *index.entry(p.map(f64::to_bits)).or_insert_with(|| {
                        verts.push(p);
                        (verts.len() - 1) as u32
                    })
                })
            })
            .collect();

        let mut bbox_min = [f64::INFINITY; 3];
        let mut bbox_max = [f64::NEG_INFINITY; 3];
        for p in &verts {
            for k in 0..3 {
                bbox_min[k] = bbox_min[k].min(p[k]);
                bbox_max[k] = bbox_max[k].max(p[k]);
            }
        }
        if verts.is_empty() {
            bbox_min = [0.0; 3];
            bbox_max = [0.0; 3];
        }

        // Volume relative to the box center (less cancellation), and area.
        let center = DVec3::from_array(bbox_min).lerp(DVec3::from_array(bbox_max), 0.5);
        let (mut volume, mut area) = (0.0, 0.0);
        for t in &tris {
            let [a, b, c] = t.map(DVec3::from_array);
            volume += (a - center).dot((b - center).cross(c - center));
            area += (b - a).cross(c - a).length() * 0.5;
        }
        volume /= 6.0;

        // Topology over welded, non-degenerate triangles.
        let mut degenerate = 0;
        // (min, max) -> (uses, uses in min->max direction)
        let mut edges: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
        let mut parent: Vec<u32> = (0..verts.len() as u32).collect();
        fn find(parent: &mut [u32], mut x: u32) -> u32 {
            while parent[x as usize] != x {
                parent[x as usize] = parent[parent[x as usize] as usize];
                x = parent[x as usize];
            }
            x
        }
        let mut faces = 0i64;
        let mut used = vec![false; verts.len()];
        for t in &ids {
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                degenerate += 1;
                continue;
            }
            faces += 1;
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                used[a as usize] = true;
                let e = edges.entry((a.min(b), a.max(b))).or_insert((0, 0));
                e.0 += 1;
                if a < b {
                    e.1 += 1;
                }
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    parent[ra as usize] = rb;
                }
            }
        }
        let open_edges = edges.values().filter(|e| e.0 == 1).count();
        let nonmanifold_edges = edges
            .values()
            .filter(|e| e.0 > 2 || (e.0 == 2 && e.1 != 1))
            .count();
        let mut roots: Vec<u32> = (0..verts.len() as u32)
            .filter(|&v| used[v as usize])
            .map(|v| find(&mut parent, v))
            .collect();
        roots.sort_unstable();
        roots.dedup();
        let v_used = used.iter().filter(|&&u| u).count() as i64;
        let euler = v_used - edges.len() as i64 + faces;

        // Fingerprints over coordinates rounded to 1e-6.
        let rounded = |p: &[f64; 3]| -> [String; 3] { p.map(round_text) };
        let parse3 = |s: &[String; 3]| -> [f64; 3] { s.clone().map(|c| c.parse().unwrap_or(0.0)) };
        let mut rverts: Vec<[f64; 3]> = verts.iter().map(|p| parse3(&rounded(p))).collect();
        rverts.sort_by(cmp3);
        rverts.dedup_by(|a, b| cmp3(a, b).is_eq());
        let mut text = String::new();
        for p in &rverts {
            let _ = writeln!(text, "{}", p.map(round_text).join(" "));
        }
        let vertex_hash = fnv1a64(text.as_bytes());
        let mut rtris: Vec<[[f64; 3]; 3]> = tris
            .iter()
            .map(|t| canonical_rotation(t.map(|p| parse3(&rounded(&p))), |a, b| cmp3(a, b).is_lt()))
            .collect();
        rtris.sort_by(|a, b| {
            cmp3(&a[0], &b[0])
                .then(cmp3(&a[1], &b[1]))
                .then(cmp3(&a[2], &b[2]))
        });
        text.clear();
        for t in &rtris {
            let line: Vec<String> = t.iter().map(|p| p.map(round_text).join(" ")).collect();
            let _ = writeln!(text, "{}", line.join(" | "));
        }
        let triangle_hash = fnv1a64(text.as_bytes());

        Metrics {
            triangles: soup.len(),
            vertices: verts.len(),
            volume,
            area,
            bbox_min,
            bbox_max,
            manifold: open_edges == 0 && nonmanifold_edges == 0 && faces > 0,
            open_edges,
            nonmanifold_edges,
            degenerate_triangles: degenerate,
            components: roots.len(),
            euler,
            genus: 1 - euler / 2,
            vertex_hash,
            triangle_hash,
            vertex_list: (rverts.len() <= VERTEX_LIST_LIMIT).then_some(rverts),
        }
    }

    pub fn to_json(&self) -> Json {
        let num = |v: f64| Json::Num(v);
        let int = |v: usize| Json::Num(v as f64);
        let arr3 = |a: [f64; 3]| Json::Arr(a.iter().map(|&v| Json::Num(v)).collect());
        let mut fields = vec![
            ("triangles".to_string(), int(self.triangles)),
            ("vertices".into(), int(self.vertices)),
            ("volume".into(), num(self.volume)),
            ("area".into(), num(self.area)),
            ("bbox_min".into(), arr3(self.bbox_min)),
            ("bbox_max".into(), arr3(self.bbox_max)),
            ("manifold".into(), Json::Bool(self.manifold)),
            ("open_edges".into(), int(self.open_edges)),
            ("nonmanifold_edges".into(), int(self.nonmanifold_edges)),
            (
                "degenerate_triangles".into(),
                int(self.degenerate_triangles),
            ),
            ("components".into(), int(self.components)),
            ("euler".into(), num(self.euler as f64)),
            ("genus".into(), num(self.genus as f64)),
            ("vertex_hash".into(), Json::Str(self.vertex_hash.clone())),
            (
                "triangle_hash".into(),
                Json::Str(self.triangle_hash.clone()),
            ),
        ];
        if let Some(list) = &self.vertex_list {
            fields.push((
                "vertex_list".into(),
                Json::Arr(list.iter().map(|&p| arr3(p)).collect()),
            ));
        }
        Json::Obj(fields)
    }

    pub fn from_json(v: &Json) -> Result<Self, String> {
        let num = |k: &str| {
            v.get(k)
                .and_then(Json::as_f64)
                .ok_or_else(|| format!("missing number '{k}'"))
        };
        let int = |k: &str| num(k).map(|n| n as usize);
        let arr3 = |j: &Json| -> Option<[f64; 3]> {
            let a = j.as_array()?;
            Some([
                a.first()?.as_f64()?,
                a.get(1)?.as_f64()?,
                a.get(2)?.as_f64()?,
            ])
        };
        let get3 = |k: &str| {
            v.get(k)
                .and_then(arr3)
                .ok_or_else(|| format!("missing vector '{k}'"))
        };
        let string = |k: &str| {
            v.get(k)
                .and_then(Json::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("missing string '{k}'"))
        };
        let vertex_list = match v.get("vertex_list") {
            Some(Json::Arr(items)) => Some(
                items
                    .iter()
                    .map(|p| arr3(p).ok_or("invalid vertex_list entry"))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            _ => None,
        };
        Ok(Metrics {
            triangles: int("triangles")?,
            vertices: int("vertices")?,
            volume: num("volume")?,
            area: num("area")?,
            bbox_min: get3("bbox_min")?,
            bbox_max: get3("bbox_max")?,
            manifold: v
                .get("manifold")
                .and_then(Json::as_bool)
                .ok_or("missing bool 'manifold'")?,
            open_edges: int("open_edges")?,
            nonmanifold_edges: int("nonmanifold_edges")?,
            degenerate_triangles: int("degenerate_triangles")?,
            components: int("components")?,
            euler: num("euler")? as i64,
            genus: num("genus")? as i64,
            vertex_hash: string("vertex_hash")?,
            triangle_hash: string("triangle_hash")?,
            vertex_list,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn cube_soup(offset: DVec3, size: f64) -> Vec<[DVec3; 3]> {
        let p = |i: u32| {
            offset
                + DVec3::new(
                    f64::from(i & 1),
                    f64::from((i >> 1) & 1),
                    f64::from((i >> 2) & 1),
                ) * size
        };
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        quads
            .iter()
            .flat_map(|q| [[p(q[0]), p(q[1]), p(q[2])], [p(q[0]), p(q[2]), p(q[3])]])
            .collect()
    }

    #[test]
    fn cube_metrics() {
        let m = Metrics::from_soup(&cube_soup(DVec3::ZERO, 10.0));
        assert_eq!((m.triangles, m.vertices), (12, 8));
        assert!((m.volume - 1000.0).abs() < 1e-9);
        assert!((m.area - 600.0).abs() < 1e-9);
        assert_eq!(m.bbox_max, [10.0; 3]);
        assert!(m.manifold);
        assert_eq!((m.components, m.euler, m.genus), (1, 2, 0));
        assert_eq!(m.vertex_list.as_ref().map(Vec::len), Some(8));
        let back = Metrics::from_json(&crate::json::parse(&m.to_json().to_pretty()).unwrap());
        assert_eq!(back.unwrap(), m);
    }

    #[test]
    fn order_independent_and_topology() {
        let mut soup = cube_soup(DVec3::ZERO, 1.0);
        soup.extend(cube_soup(DVec3::new(3.0, 0.0, 0.0), 1.0));
        let a = Metrics::from_soup(&soup);
        soup.reverse();
        for t in &mut soup {
            t.rotate_left(1);
        }
        let b = Metrics::from_soup(&soup);
        assert_eq!(a, b);
        assert_eq!((a.components, a.genus), (2, -1));
        // An open mesh.
        soup.pop();
        let c = Metrics::from_soup(&soup);
        assert!(!c.manifold);
        assert_eq!(c.open_edges, 3);
        assert_ne!(c.triangle_hash, a.triangle_hash);
        assert_eq!(c.vertex_hash, a.vertex_hash);
    }

    #[test]
    fn rounding_hides_tiny_noise() {
        let a = Metrics::from_soup(&cube_soup(DVec3::ZERO, 1.0));
        let b = Metrics::from_soup(&cube_soup(DVec3::splat(1e-9), 1.0));
        assert_eq!(a.vertex_hash, b.vertex_hash);
        assert_eq!(a.triangle_hash, b.triangle_hash);
        assert_eq!(round_text(-1e-9), "0.000000");
    }
}
