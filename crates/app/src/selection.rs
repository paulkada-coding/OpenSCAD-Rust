//! Click on the model → code: which source produced each triangle of the
//! displayed model, the current selection, and its panel.
//!
//! A picked triangle has a chain of sources, from the top-level statement
//! down to the primitive (e.g. `table();` › `leg(3);` › `translate(…)` ›
//! `cube(1);`). One link of the chain is selected (the primitive at
//! first); every triangle produced through that code is highlighted, so
//! picking a statement inside a module shows all of its instances.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;
use geometry::Mesh;
use geometry::csg::SourceRef;
use mesher::RenderOutput;
use scad_eval::Evaluator;

/// Longest code excerpt kept per source, in lines.
const MAX_SNIPPET_LINES: usize = 12;
/// Longest one-line summary shown in the chain, in characters.
const MAX_SUMMARY_CHARS: usize = 60;
/// Deepest indentation of the chain, in links.
const MAX_INDENT: usize = 6;

/// A source span resolved to something a person can read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub path: PathBuf,
    /// 1-based.
    pub line: u32,
    pub end_line: u32,
    /// The code of the span, at most [`MAX_SNIPPET_LINES`] lines.
    pub snippet: String,
}

impl SourceLocation {
    fn file_name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    /// `name.scad, line 12` or `name.scad, lines 12–15`.
    pub fn describe(&self) -> String {
        if self.end_line > self.line {
            format!(
                "{}, lines {}–{}",
                self.file_name(),
                self.line,
                self.end_line
            )
        } else {
            format!("{}, line {}", self.file_name(), self.line)
        }
    }

    /// First line of the code, shortened.
    pub fn summary(&self) -> String {
        let first = self.snippet.lines().next().unwrap_or("").trim();
        let multi_line = self.snippet.trim_end().contains('\n');
        let mut summary: String = first.chars().take(MAX_SUMMARY_CHARS).collect();
        if multi_line || summary.len() < first.len() {
            summary.push('…');
        }
        summary
    }
}

/// Which code produced each triangle of the displayed model.
#[derive(Debug, Default)]
pub struct Provenance {
    /// Per triangle: index into `chains`, or `u32::MAX` when unknown.
    triangle_sources: Vec<u32>,
    /// Chain of sources of each entry, outermost first.
    chains: Vec<Vec<SourceRef>>,
    locations: HashMap<SourceRef, SourceLocation>,
}

impl Provenance {
    /// Takes the provenance out of a render (the app needs it nowhere
    /// else) and resolves every source while the evaluator, which owns the
    /// source texts, is still alive. `None` when nothing is traced (2D
    /// results).
    pub fn take(output: &mut RenderOutput, evaluator: &Evaluator) -> Option<Self> {
        if output.triangle_sources.is_empty() {
            return None;
        }
        let triangle_sources = std::mem::take(&mut output.triangle_sources);
        let chains = std::mem::take(&mut output.source_paths);
        let mut locations = HashMap::new();
        for &source in chains.iter().flatten() {
            if locations.contains_key(&source) {
                continue;
            }
            if let Some(resolved) = evaluator.resolve_source(source) {
                let snippet = resolved
                    .snippet
                    .lines()
                    .take(MAX_SNIPPET_LINES)
                    .collect::<Vec<_>>()
                    .join("\n");
                let truncated = resolved.snippet.lines().count() > MAX_SNIPPET_LINES;
                locations.insert(
                    source,
                    SourceLocation {
                        path: resolved.path.to_path_buf(),
                        line: resolved.start_line_col.0,
                        end_line: resolved.end_line_col.0,
                        snippet: if truncated {
                            snippet + "\n…"
                        } else {
                            snippet
                        },
                    },
                );
            }
        }
        Some(Self {
            triangle_sources,
            chains,
            locations,
        })
    }

    /// The selection for a picked triangle, with the primitive selected.
    pub fn select(&self, triangle: usize) -> Option<Selection> {
        let index = *self.triangle_sources.get(triangle)?;
        let chain = self.chains.get(index as usize)?;
        let links: Vec<Link> = chain
            .iter()
            .filter_map(|s| {
                Some(Link {
                    source: *s,
                    location: self.locations.get(s)?.clone(),
                })
            })
            .collect();
        (!links.is_empty()).then(|| Selection {
            level: links.len() - 1,
            links,
            triangles: 0,
        })
    }

    /// The same selection in a newer render, when the code it points to is
    /// unchanged (a new Customizer value, an edit elsewhere in the file
    /// that did not move it).
    pub fn carry_over(&self, old: &Selection) -> Option<Selection> {
        let same = |chain: &Vec<SourceRef>| {
            chain.len() == old.links.len()
                && chain.iter().zip(&old.links).all(|(s, link)| {
                    *s == link.source && self.locations.get(s) == Some(&link.location)
                })
        };
        self.chains.iter().any(same).then(|| Selection {
            links: old.links.clone(),
            level: old.level,
            triangles: 0,
        })
    }

    /// The triangles of `model` produced through `source`.
    pub fn highlight(&self, model: &Mesh, source: SourceRef) -> Mesh {
        let through: Vec<bool> = self
            .chains
            .iter()
            .map(|chain| chain.contains(&source))
            .collect();
        let triangles = model
            .triangles
            .iter()
            .zip(&self.triangle_sources)
            .filter(|&(_, &index)| through.get(index as usize).copied().unwrap_or(false))
            .map(|(tri, _)| *tri)
            .collect();
        Mesh {
            vertices: model.vertices.clone(),
            triangles,
        }
    }
}

/// One step of a chain of sources.
#[derive(Debug, Clone)]
pub struct Link {
    pub source: SourceRef,
    pub location: SourceLocation,
}

/// The picked code.
#[derive(Debug, Clone)]
pub struct Selection {
    /// From the top-level statement down to the primitive.
    pub links: Vec<Link>,
    /// The selected link; its code is highlighted.
    pub level: usize,
    /// Number of highlighted triangles, for the panel.
    pub triangles: usize,
}

impl Selection {
    pub fn current(&self) -> &Link {
        &self.links[self.level.min(self.links.len() - 1)]
    }
}

/// What the user did in the selection panel.
pub enum SelectionEvent {
    None,
    /// Another link of the chain was chosen.
    Level(usize),
    Clear,
}

/// The "Selection" section of the model panel.
pub fn selection_ui(ui: &mut egui::Ui, selection: Option<&Selection>) -> SelectionEvent {
    let mut event = SelectionEvent::None;
    ui.horizontal(|ui| {
        ui.strong("Selection");
        if selection.is_some() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Clear")
                    .on_hover_text("Clear the selection (Esc)")
                    .clicked()
                {
                    event = SelectionEvent::Clear;
                }
            });
        }
    });
    let Some(selection) = selection else {
        ui.weak("Click the model to find the code that made it.");
        return event;
    };

    ui.add_space(2.0);
    for (i, link) in selection.links.iter().enumerate() {
        ui.horizontal(|ui| {
            // Deep chains (libraries) stop indenting, to keep room for code.
            ui.add_space(8.0 * i.min(MAX_INDENT) as f32);
            let text = egui::RichText::new(format!(
                "{:>4}  {}",
                link.location.line,
                link.location.summary()
            ))
            .monospace();
            // Truncated, so long code does not widen the panel.
            if ui
                .add(egui::Button::selectable(i == selection.level, text).truncate())
                .on_hover_text(format!(
                    "{}\n{}",
                    link.location.describe(),
                    link.location.snippet
                ))
                .clicked()
                && i != selection.level
            {
                event = SelectionEvent::Level(i);
            }
        });
    }

    let current = selection.current();
    ui.add_space(6.0);
    ui.label(current.location.describe());
    ui.weak(format!(
        "{} highlighted triangle{}",
        selection.triangles,
        if selection.triangles == 1 { "" } else { "s" }
    ));
    ui.add_space(4.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(egui::RichText::new(&current.location.snippet).monospace()).wrap());
    });
    event
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use mesher::{RenderOptions, Shape};
    use scad_eval::EvalOptions;

    fn render(code: &str) -> (Mesh, Provenance) {
        let mut evaluator = Evaluator::new(EvalOptions::default());
        let (nodes, error) = evaluator.run(Path::new("model.scad"), code);
        assert!(error.is_none());
        let mut output = mesher::render(&nodes, &RenderOptions::default()).expect("render");
        let provenance = Provenance::take(&mut output, &evaluator).expect("provenance");
        match output.shape {
            Shape::Solid(mesh) => (mesh, provenance),
            other => panic!("not a solid: {other:?}"),
        }
    }

    const TABLE: &str = "module leg(x) translate([x, 0, 0]) cube(1);\n\
                         module table() {\n  leg(0);\n  leg(3);\n}\n\
                         table();\n\
                         translate([0, 5, 0])\n  sphere(1, $fn = 8);\n";

    /// A triangle of the leg at x = 3.
    fn second_leg(mesh: &Mesh) -> usize {
        (0..mesh.triangle_count())
            .find(|&i| mesh.triangle(i).iter().all(|p| p.x >= 3.0 && p.y < 2.0))
            .expect("leg triangle")
    }

    #[test]
    fn picking_resolves_the_chain_of_calls() {
        let (mesh, provenance) = render(TABLE);
        let selection = provenance.select(second_leg(&mesh)).expect("selection");
        let summaries: Vec<(u32, String)> = selection
            .links
            .iter()
            .map(|l| (l.location.line, l.location.summary()))
            .collect();
        assert_eq!(
            summaries,
            [
                (6, "table();".to_string()),
                (4, "leg(3);".to_string()),
                (1, "translate([x, 0, 0]) cube(1);".to_string()),
                (1, "cube(1);".to_string()),
            ]
        );
        assert_eq!(selection.level, 3);
        assert_eq!(
            selection.current().location.describe(),
            "model.scad, line 1"
        );
        // A statement spanning lines is summarized by its first one.
        let sphere = (0..mesh.triangle_count())
            .find(|&i| mesh.triangle(i)[0].y > 3.0)
            .expect("sphere triangle");
        let selection = provenance.select(sphere).expect("selection");
        let outer = &selection.links[0].location;
        assert_eq!(outer.summary(), "translate([0, 5, 0])…");
        assert_eq!(outer.describe(), "model.scad, lines 7–8");
    }

    #[test]
    fn highlight_follows_the_selected_link() {
        let (mesh, provenance) = render(TABLE);
        let selection = provenance.select(second_leg(&mesh)).expect("selection");
        let count = |level: usize| {
            provenance
                .highlight(&mesh, selection.links[level].source)
                .triangle_count()
        };
        // `cube(1)` and the `translate` in `leg`: both legs.
        assert_eq!(count(3), 24);
        assert_eq!(count(2), 24);
        // `leg(3);`: this leg only.
        assert_eq!(count(1), 12);
        // `table();`: the whole table, not the sphere.
        assert_eq!(count(0), 24);
        assert!(count(0) < mesh.triangle_count());
    }

    #[test]
    fn selection_survives_a_new_render_of_the_same_code() {
        let (mesh, provenance) = render(TABLE);
        let selection = provenance.select(second_leg(&mesh)).expect("selection");
        let (_, again) = render(TABLE);
        let kept = again.carry_over(&selection).expect("kept");
        assert_eq!(kept.level, selection.level);
        // Once the code moves, the old selection is meaningless.
        let (_, moved) = render(&format!("// a new first line\n{TABLE}"));
        assert!(moved.carry_over(&selection).is_none());
    }
}
