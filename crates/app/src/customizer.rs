//! Desktop Customizer panel: renders interactive controls from extracted
//! `.scad` parameter annotations, manages `<stem>.json` presets, debounces
//! live background re-evaluations, and writes modified values back to disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use scad_customizer::{
    DEFAULT_GROUP, NumberOption, ParamConstraint, ParamValue, Parameter, PresetFile, StringOption,
    VectorRange, extract_parameters, preset_path_for_scad, read_preset_file,
    resolve_preset_overrides, write_preset_file,
};
use serde::{Deserialize, Serialize};

use crate::worker::modified_time;

pub const STORAGE_KEY: &str = "customizer_state";

/// Interval between live background re-evaluations while dragging a control.
const DEBOUNCE_DELAY: Duration = Duration::from_millis(100);

const DEFAULT_PRESET_LABEL: &str = "<default>";

/// Per-file Customizer overrides persisted across sessions via `eframe`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedCustomizer {
    pub files: BTreeMap<String, PersistedFileState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedFileState {
    pub selected_preset: Option<String>,
    /// Only parameters modified away from the `.scad` default are persisted,
    /// so untouched parameters always follow edits made directly to the file.
    pub overrides: BTreeMap<String, ParamValue>,
}

impl PersistedCustomizer {
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        storage
            .and_then(|s| eframe::get_value(s, STORAGE_KEY))
            .unwrap_or_default()
    }
}

/// High-level event returned to [`crate::app::ViewerApp`] after drawing the
/// Customizer panel or ticking its debounce timer.
#[derive(Debug)]
pub enum CustomizerEvent {
    None,
    /// Trigger a background re-evaluation with [`CustomizerState::active_defines`].
    Evaluate,
    /// Rewrote modified literals into the `.scad` file on disk.
    AppliedToFile {
        count: usize,
        new_mtime: Option<SystemTime>,
    },
    /// Show an informational or error message in the status bar.
    Status {
        is_error: bool,
        message: String,
    },
}

#[derive(Debug, Clone)]
enum PresetModal {
    SaveAs { name: String },
    Rename { old_name: String, new_name: String },
}

/// Active Customizer state for the currently open `.scad` document.
pub struct CustomizerState {
    scad_path: PathBuf,
    preset_path: PathBuf,
    preset_mtime: Option<SystemTime>,
    parameters: Vec<Parameter>,
    values: Vec<ParamValue>,
    presets: PresetFile,
    selected_preset: Option<String>,
    modal: Option<PresetModal>,
    dirty_since: Option<Instant>,
    immediate_eval: bool,
}

impl CustomizerState {
    /// Loads parameters and `<stem>.json` presets for `scad_path`, restoring
    /// any saved per-file overrides from `persisted`.
    pub fn load_for_file(scad_path: &Path, persisted: &PersistedCustomizer) -> Self {
        let preset_path = preset_path_for_scad(scad_path);
        let preset_mtime = modified_time(&preset_path);
        let presets = read_preset_file(&preset_path).unwrap_or_default();
        let parameters = std::fs::read_to_string(scad_path)
            .map(|src| extract_parameters(&src).parameters)
            .unwrap_or_default();

        let key = canonical_file_key(scad_path);
        let saved = persisted.files.get(&key);

        let selected_preset = saved
            .and_then(|s| s.selected_preset.clone())
            .filter(|name| presets.contains_set(name));

        let mut values: Vec<ParamValue> = parameters.iter().map(|p| p.default.clone()).collect();

        if let Some(preset_name) = &selected_preset {
            for (name, val) in resolve_preset_overrides(&presets, preset_name, &parameters) {
                if let Some(idx) = parameters.iter().position(|p| p.name == name) {
                    values[idx] = val;
                }
            }
        }
        if let Some(saved) = saved {
            for (idx, param) in parameters.iter().enumerate() {
                if let Some(saved_val) = saved.overrides.get(&param.name)
                    && let Some(valid) = sanitize_value(param, saved_val)
                {
                    values[idx] = valid;
                }
            }
        }

        Self {
            scad_path: scad_path.to_path_buf(),
            preset_path,
            preset_mtime,
            parameters,
            values,
            presets,
            selected_preset,
            modal: None,
            dirty_since: None,
            immediate_eval: false,
        }
    }

    /// Re-extracts parameters when the `.scad` (or companion `.json`) file
    /// changes on disk, keeping modified values whose parameter still exists
    /// with the same type.
    pub fn reload_from_disk(&mut self) {
        let modified: BTreeMap<String, ParamValue> = self
            .parameters
            .iter()
            .zip(&self.values)
            .filter(|(p, v)| !v.values_equal(&p.default))
            .map(|(p, v)| (p.name.clone(), v.clone()))
            .collect();

        if let Ok(src) = std::fs::read_to_string(&self.scad_path) {
            self.parameters = extract_parameters(&src).parameters;
        }
        self.refresh_presets_if_changed();

        self.values = self
            .parameters
            .iter()
            .map(|param| {
                modified
                    .get(&param.name)
                    .and_then(|old| sanitize_value(param, old))
                    .unwrap_or_else(|| param.default.clone())
            })
            .collect();
    }

    /// Refreshes `<stem>.json` if it was created, edited, or deleted externally.
    pub fn refresh_presets_if_changed(&mut self) {
        let now_mtime = modified_time(&self.preset_path);
        if now_mtime != self.preset_mtime {
            self.preset_mtime = now_mtime;
            self.presets = read_preset_file(&self.preset_path).unwrap_or_default();
            if self
                .selected_preset
                .as_deref()
                .is_some_and(|name| !self.presets.contains_set(name))
            {
                self.selected_preset = None;
            }
        }
    }

    /// Saves current per-file overrides into `persisted`.
    pub fn persist_into(&self, persisted: &mut PersistedCustomizer) {
        let key = canonical_file_key(&self.scad_path);
        let overrides: BTreeMap<String, ParamValue> = self
            .parameters
            .iter()
            .zip(&self.values)
            .filter(|(p, v)| !v.values_equal(&p.default))
            .map(|(p, v)| (p.name.clone(), v.clone()))
            .collect();

        if overrides.is_empty() && self.selected_preset.is_none() {
            persisted.files.remove(&key);
        } else {
            persisted.files.insert(
                key,
                PersistedFileState {
                    selected_preset: self.selected_preset.clone(),
                    overrides,
                },
            );
        }
    }

    /// Returns `(name, scad_literal)` pairs for every modified parameter.
    pub fn active_defines(&self) -> Vec<(String, String)> {
        self.parameters
            .iter()
            .zip(&self.values)
            .filter(|(p, v)| !v.values_equal(&p.default))
            .map(|(p, v)| (p.name.clone(), v.to_scad_literal()))
            .collect()
    }

    pub fn has_modifications(&self) -> bool {
        self.parameters
            .iter()
            .zip(&self.values)
            .any(|(p, v)| !v.values_equal(&p.default))
    }

    /// Remaining time until a pending debounced evaluation is due, so `OnDemand`
    /// frame pacing wakes up cleanly.
    pub fn pending_repaint_delay(&self) -> Option<Duration> {
        if self.immediate_eval {
            return Some(Duration::ZERO);
        }
        let since = self.dirty_since?;
        Some(DEBOUNCE_DELAY.saturating_sub(since.elapsed()))
    }

    /// Consumes a due evaluation request if either an immediate change occurred
    /// or the debounce interval has elapsed.
    pub fn take_due_evaluation(&mut self) -> bool {
        if self.immediate_eval {
            self.immediate_eval = false;
            self.dirty_since = None;
            return true;
        }
        if let Some(since) = self.dirty_since
            && since.elapsed() >= DEBOUNCE_DELAY
        {
            self.dirty_since = None;
            return true;
        }
        false
    }

    fn mark_debounced_change(&mut self) {
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    fn mark_immediate_change(&mut self) {
        self.immediate_eval = true;
        self.dirty_since = None;
    }

    /// Draws the Customizer panel contents and returns any immediate event.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> CustomizerEvent {
        let mut event = CustomizerEvent::None;

        ui.horizontal(|ui| {
            ui.heading("Parameters");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let modified = self.has_modifications();
                if ui
                    .add_enabled(modified, egui::Button::new("Apply to file").small())
                    .on_hover_text(
                        "Write modified parameter literals into the .scad file (preserving comments and formatting)",
                    )
                    .clicked()
                {
                    event = self.apply_to_file();
                }
                if ui
                    .add_enabled(modified, egui::Button::new("↺ Reset").small())
                    .on_hover_text("Reset all parameters to their .scad defaults")
                    .clicked()
                {
                    self.reset_all();
                    self.selected_preset = None;
                    self.mark_immediate_change();
                }
            });
        });

        if self.parameters.is_empty() {
            ui.add_space(6.0);
            ui.weak("No customizable parameters in this file.");
            return event;
        }

        ui.add_space(4.0);
        if let Some(preset_event) = self.presets_bar_ui(ui) {
            event = preset_event;
        }
        ui.separator();

        let group_order = self.group_names_in_order();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for group_name in &group_order {
                    egui::CollapsingHeader::new(egui::RichText::new(group_name).strong())
                        .id_salt(("customizer_group", group_name))
                        .default_open(true)
                        .show(ui, |ui| {
                            let indices: Vec<usize> = self
                                .parameters
                                .iter()
                                .enumerate()
                                .filter(|(_, p)| {
                                    p.group.as_deref().unwrap_or(DEFAULT_GROUP) == group_name
                                })
                                .map(|(idx, _)| idx)
                                .collect();
                            for (pos, idx) in indices.into_iter().enumerate() {
                                if pos > 0 {
                                    ui.add_space(6.0);
                                }
                                self.parameter_row_ui(ui, idx);
                            }
                        });
                }
            });

        if self.take_due_evaluation() && matches!(event, CustomizerEvent::None) {
            event = CustomizerEvent::Evaluate;
        }
        event
    }

    fn presets_bar_ui(&mut self, ui: &mut egui::Ui) -> Option<CustomizerEvent> {
        let mut status_event = None;

        ui.horizontal(|ui| {
            ui.label("Preset");
            let current_label = self
                .selected_preset
                .as_deref()
                .unwrap_or(DEFAULT_PRESET_LABEL)
                .to_string();
            let set_names: Vec<String> = self.presets.set_names().map(str::to_string).collect();
            let mut chosen = self.selected_preset.clone();
            egui::ComboBox::from_id_salt("customizer_preset_combo")
                .width((ui.available_width() - 4.0).max(90.0))
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut chosen, None, DEFAULT_PRESET_LABEL);
                    for name in &set_names {
                        ui.selectable_value(&mut chosen, Some(name.clone()), name);
                    }
                });
            if chosen != self.selected_preset {
                self.select_preset(chosen);
                self.mark_immediate_change();
            }
        });

        ui.horizontal_wrapped(|ui| {
            let has_named = self.selected_preset.is_some();
            if ui
                .small_button("Save")
                .on_hover_text("Save current parameter values to the selected preset")
                .clicked()
            {
                if let Some(name) = self.selected_preset.clone() {
                    status_event = Some(self.save_preset(&name));
                } else {
                    self.modal = Some(PresetModal::SaveAs {
                        name: String::new(),
                    });
                }
            }
            if ui
                .small_button("Save as…")
                .on_hover_text("Create a new named preset in the companion .json file")
                .clicked()
            {
                self.modal = Some(PresetModal::SaveAs {
                    name: String::new(),
                });
            }
            if ui
                .add_enabled(has_named, egui::Button::new("Rename…").small())
                .clicked()
                && let Some(old_name) = self.selected_preset.clone()
            {
                self.modal = Some(PresetModal::Rename {
                    new_name: old_name.clone(),
                    old_name,
                });
            }
            if ui
                .add_enabled(has_named, egui::Button::new("Delete").small())
                .clicked()
                && let Some(name) = self.selected_preset.take()
            {
                status_event = Some(self.delete_preset(&name));
            }
        });

        if let Some(modal) = &mut self.modal {
            let mut close_modal = false;
            let mut confirm_save_as = None;
            let mut confirm_rename = None;

            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let buf = match modal {
                    PresetModal::SaveAs { name } => name,
                    PresetModal::Rename { new_name, .. } => new_name,
                };
                let resp = ui.add(
                    egui::TextEdit::singleline(buf)
                        .hint_text("Preset name")
                        .desired_width(140.0),
                );
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let trimmed = buf.trim().to_string();
                let valid = !trimmed.is_empty() && trimmed != DEFAULT_PRESET_LABEL;

                if (ui
                    .add_enabled(valid, egui::Button::new("OK").small())
                    .clicked()
                    || enter)
                    && valid
                {
                    match modal {
                        PresetModal::SaveAs { .. } => confirm_save_as = Some(trimmed),
                        PresetModal::Rename { old_name, .. } => {
                            confirm_rename = Some((old_name.clone(), trimmed));
                        }
                    }
                    close_modal = true;
                }
                if ui.small_button("Cancel").clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    close_modal = true;
                }
            });

            if close_modal {
                self.modal = None;
            }
            if let Some(new_name) = confirm_save_as {
                self.selected_preset = Some(new_name.clone());
                status_event = Some(self.save_preset(&new_name));
            }
            if let Some((old_name, new_name)) = confirm_rename {
                status_event = Some(self.rename_preset(&old_name, &new_name));
            }
        }

        status_event
    }

    fn parameter_row_ui(&mut self, ui: &mut egui::Ui, idx: usize) {
        let is_modified = !self.values[idx].values_equal(&self.parameters[idx].default);
        let default_tip = format!(
            "Default: {}",
            self.parameters[idx].default.to_scad_literal()
        );

        ui.push_id(idx, |ui| {
            // Boolean parameters fit cleanly on a single line with their reset button.
            if matches!(
                self.parameters[idx].constraint,
                ParamConstraint::UnconstrainedBool
            ) && let ParamValue::Bool(mut checked) = self.values[idx]
            {
                ui.horizontal(|ui| {
                    let label = param_label_text(&self.parameters[idx].name, is_modified, ui);
                    let resp = ui.checkbox(&mut checked, label).on_hover_text(&default_tip);
                    if resp.changed() {
                        self.values[idx] = ParamValue::Bool(checked);
                        self.mark_immediate_change();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if is_modified
                            && ui
                                .small_button("↺")
                                .on_hover_text("Reset to default")
                                .clicked()
                        {
                            self.values[idx] = self.parameters[idx].default.clone();
                            self.mark_immediate_change();
                        }
                    });
                });
                if let Some(caption) = &self.parameters[idx].caption {
                    ui.weak(caption);
                }
                return;
            }

            // Header line: parameter name + per-parameter reset button.
            ui.horizontal(|ui| {
                let label = param_label_text(&self.parameters[idx].name, is_modified, ui);
                ui.label(label).on_hover_text(&default_tip);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_modified
                        && ui
                            .small_button("↺")
                            .on_hover_text("Reset to default")
                            .clicked()
                    {
                        self.values[idx] = self.parameters[idx].default.clone();
                        self.mark_immediate_change();
                    }
                });
            });

            // Control widget(s) according to constraint.
            let constraint = self.parameters[idx].constraint.clone();
            match constraint {
                ParamConstraint::UnconstrainedBool => {}
                ParamConstraint::NumberRange { min, max, step } => {
                    if let ParamValue::Number(mut val) = self.values[idx] {
                        let decimals = step_decimals(step);
                        let lo = min.min(max);
                        let hi = min.max(max);
                        let mut slider = egui::Slider::new(&mut val, lo..=hi);
                        if step > 0.0 {
                            slider = slider.step_by(step);
                        }
                        if let Some(dec) = decimals {
                            slider = slider.max_decimals(dec);
                        }
                        let resp = ui.add(slider);
                        if resp.changed() {
                            val = clean_step_float(val, decimals);
                            self.values[idx] = ParamValue::Number(val);
                            if resp.drag_stopped() || resp.lost_focus() {
                                self.mark_immediate_change();
                            } else {
                                self.mark_debounced_change();
                            }
                        } else if resp.drag_stopped() && self.dirty_since.is_some() {
                            self.mark_immediate_change();
                        }
                    }
                }
                ParamConstraint::NumberSpinbox { step } => {
                    if let ParamValue::Number(mut val) = self.values[idx] {
                        let speed = if step.abs() > 0.0 { step.abs() } else { 1.0 };
                        let decimals = step_decimals(speed);
                        let mut drag = egui::DragValue::new(&mut val).speed(speed);
                        if let Some(dec) = decimals {
                            drag = drag.max_decimals(dec);
                        }
                        let resp = ui.add(drag);
                        if resp.changed() {
                            val = clean_step_float(val, decimals);
                            self.values[idx] = ParamValue::Number(val);
                            if resp.drag_stopped() || resp.lost_focus() {
                                self.mark_immediate_change();
                            } else {
                                self.mark_debounced_change();
                            }
                        } else if resp.drag_stopped() && self.dirty_since.is_some() {
                            self.mark_immediate_change();
                        }
                    }
                }
                ParamConstraint::NumberEnum { options } => {
                    if let ParamValue::Number(current) = self.values[idx]
                        && let Some(chosen) = number_enum_combo(ui, current, &options)
                    {
                        self.values[idx] = ParamValue::Number(chosen);
                        self.mark_immediate_change();
                    }
                }
                ParamConstraint::StringText { max_length } => {
                    if let ParamValue::String(mut text) = self.values[idx].clone() {
                        let mut edit =
                            egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY);
                        if let Some(limit) = max_length {
                            edit = edit.char_limit(limit);
                        }
                        let resp = ui.add(edit);
                        if resp.changed() {
                            if let Some(limit) = max_length {
                                text = text.chars().take(limit).collect();
                            }
                            self.values[idx] = ParamValue::String(text);
                            self.mark_debounced_change();
                        }
                        if resp.lost_focus() && self.dirty_since.is_some() {
                            self.mark_immediate_change();
                        }
                    }
                }
                ParamConstraint::StringEnum { options } => {
                    if let ParamValue::String(current) = &self.values[idx]
                        && let Some(chosen) = string_enum_combo(ui, current, &options)
                    {
                        self.values[idx] = ParamValue::String(chosen);
                        self.mark_immediate_change();
                    }
                }
                ParamConstraint::Vector { range } => {
                    if let ParamValue::Vector(mut elems) = self.values[idx].clone() {
                        let mut changed = false;
                        let mut finished = false;
                        ui.horizontal(|ui| {
                            let count = elems.len().max(1) as f32;
                            let item_w =
                                ((ui.available_width() - (count - 1.0) * 6.0) / count).max(42.0);
                            for elem in &mut elems {
                                let (speed, decimals) = match range {
                                    Some(r) => {
                                        let sp = if r.step.abs() > 0.0 {
                                            r.step.abs()
                                        } else {
                                            1.0
                                        };
                                        (sp, step_decimals(sp))
                                    }
                                    None => (0.1, None),
                                };
                                let mut drag = egui::DragValue::new(elem).speed(speed);
                                if let Some(VectorRange { min, max, .. }) = range {
                                    drag = drag.range(min.min(max)..=min.max(max));
                                }
                                if let Some(dec) = decimals {
                                    drag = drag.max_decimals(dec);
                                }
                                let resp = ui.add_sized([item_w, 20.0], drag);
                                if resp.changed() {
                                    *elem = clean_step_float(*elem, decimals);
                                    changed = true;
                                }
                                if resp.drag_stopped() || resp.lost_focus() {
                                    finished = true;
                                }
                            }
                        });
                        if changed {
                            self.values[idx] = ParamValue::Vector(elems);
                            if finished {
                                self.mark_immediate_change();
                            } else {
                                self.mark_debounced_change();
                            }
                        } else if finished && self.dirty_since.is_some() {
                            self.mark_immediate_change();
                        }
                    }
                }
            }

            if let Some(caption) = &self.parameters[idx].caption {
                ui.weak(caption);
            }
        });
    }

    fn reset_all(&mut self) {
        for (val, param) in self.values.iter_mut().zip(&self.parameters) {
            *val = param.default.clone();
        }
    }

    fn select_preset(&mut self, preset: Option<String>) {
        self.reset_all();
        if let Some(name) = &preset {
            for (param_name, val) in resolve_preset_overrides(&self.presets, name, &self.parameters)
            {
                if let Some(idx) = self.parameters.iter().position(|p| p.name == param_name) {
                    self.values[idx] = val;
                }
            }
        }
        self.selected_preset = preset;
    }

    fn current_named_values(&self) -> Vec<(String, ParamValue)> {
        self.parameters
            .iter()
            .zip(&self.values)
            .map(|(p, v)| (p.name.clone(), v.clone()))
            .collect()
    }

    fn save_preset(&mut self, name: &str) -> CustomizerEvent {
        let entries = self.current_named_values();
        self.presets.upsert_set(name.to_string(), &entries);
        match write_preset_file(&self.preset_path, &self.presets) {
            Ok(()) => {
                self.preset_mtime = modified_time(&self.preset_path);
                CustomizerEvent::Status {
                    is_error: false,
                    message: format!("Saved preset '{name}'"),
                }
            }
            Err(err) => CustomizerEvent::Status {
                is_error: true,
                message: format!("Failed to save preset: {err}"),
            },
        }
    }

    fn rename_preset(&mut self, old_name: &str, new_name: &str) -> CustomizerEvent {
        if !self.presets.rename_set(old_name, new_name.to_string()) {
            return CustomizerEvent::None;
        }
        self.selected_preset = Some(new_name.to_string());
        match write_preset_file(&self.preset_path, &self.presets) {
            Ok(()) => {
                self.preset_mtime = modified_time(&self.preset_path);
                CustomizerEvent::Status {
                    is_error: false,
                    message: format!("Renamed preset '{old_name}' to '{new_name}'"),
                }
            }
            Err(err) => CustomizerEvent::Status {
                is_error: true,
                message: format!("Failed to save preset file: {err}"),
            },
        }
    }

    fn delete_preset(&mut self, name: &str) -> CustomizerEvent {
        if !self.presets.delete_set(name) {
            return CustomizerEvent::None;
        }
        match write_preset_file(&self.preset_path, &self.presets) {
            Ok(()) => {
                self.preset_mtime = modified_time(&self.preset_path);
                CustomizerEvent::Status {
                    is_error: false,
                    message: format!("Deleted preset '{name}'"),
                }
            }
            Err(err) => CustomizerEvent::Status {
                is_error: true,
                message: format!("Failed to update preset file: {err}"),
            },
        }
    }

    fn apply_to_file(&mut self) -> CustomizerEvent {
        let updates = self.active_defines();
        if updates.is_empty() {
            return CustomizerEvent::None;
        }
        let source = match std::fs::read_to_string(&self.scad_path) {
            Ok(s) => s,
            Err(err) => {
                return CustomizerEvent::Status {
                    is_error: true,
                    message: format!("Cannot read '{}': {err}", self.scad_path.display()),
                };
            }
        };
        let refs: Vec<(&str, &str)> = updates
            .iter()
            .map(|(name, lit)| (name.as_str(), lit.as_str()))
            .collect();
        let rewritten = match scad_syntax::replace_assignment_values(&source, &refs) {
            Ok(s) => s,
            Err(err) => {
                return CustomizerEvent::Status {
                    is_error: true,
                    message: format!("Cannot apply parameters to file: {err}"),
                };
            }
        };
        if let Err(err) = std::fs::write(&self.scad_path, &rewritten) {
            return CustomizerEvent::Status {
                is_error: true,
                message: format!("Cannot write '{}': {err}", self.scad_path.display()),
            };
        }
        self.parameters = extract_parameters(&rewritten).parameters;
        self.values = self.parameters.iter().map(|p| p.default.clone()).collect();
        self.dirty_since = None;
        self.immediate_eval = false;
        CustomizerEvent::AppliedToFile {
            count: updates.len(),
            new_mtime: modified_time(&self.scad_path),
        }
    }

    fn group_names_in_order(&self) -> Vec<String> {
        let mut groups = Vec::new();
        for param in &self.parameters {
            let g = param.group.as_deref().unwrap_or(DEFAULT_GROUP);
            if !groups.iter().any(|existing| existing == g) {
                groups.push(g.to_string());
            }
        }
        groups
    }
}

fn canonical_file_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn sanitize_value(param: &Parameter, candidate: &ParamValue) -> Option<ParamValue> {
    if !candidate.same_kind(&param.default) {
        return None;
    }
    match (candidate, &param.constraint) {
        (ParamValue::Number(v), ParamConstraint::NumberRange { min, max, .. }) => {
            Some(ParamValue::Number(v.clamp(min.min(*max), min.max(*max))))
        }
        (ParamValue::Number(v), ParamConstraint::NumberEnum { options }) => options
            .iter()
            .find(|o| o.value == *v)
            .map(|o| ParamValue::Number(o.value)),
        (ParamValue::String(s), ParamConstraint::StringEnum { options }) => options
            .iter()
            .find(|o| o.value == *s)
            .map(|o| ParamValue::String(o.value.clone())),
        (
            ParamValue::String(s),
            ParamConstraint::StringText {
                max_length: Some(limit),
            },
        ) => Some(ParamValue::String(s.chars().take(*limit).collect())),
        (ParamValue::Vector(elems), ParamConstraint::Vector { range: Some(r) }) => {
            let lo = r.min.min(r.max);
            let hi = r.min.max(r.max);
            Some(ParamValue::Vector(
                elems.iter().map(|x| x.clamp(lo, hi)).collect(),
            ))
        }
        _ => Some(candidate.clone()),
    }
}

fn param_label_text(name: &str, is_modified: bool, ui: &egui::Ui) -> egui::RichText {
    if is_modified {
        egui::RichText::new(format!("{name} •"))
            .strong()
            .color(ui.visuals().selection.stroke.color)
    } else {
        egui::RichText::new(name)
    }
}

fn number_enum_combo(ui: &mut egui::Ui, current: f64, options: &[NumberOption]) -> Option<f64> {
    let selected_label = options
        .iter()
        .find(|o| o.value == current)
        .map_or_else(|| current.to_string(), |o| o.name.clone());
    let mut chosen = current;
    let mut changed = false;
    egui::ComboBox::from_id_salt("num_enum")
        .width(ui.available_width())
        .selected_text(selected_label)
        .show_ui(ui, |ui| {
            for opt in options {
                if ui
                    .selectable_value(&mut chosen, opt.value, &opt.name)
                    .changed()
                {
                    changed = true;
                }
            }
        });
    changed.then_some(chosen)
}

fn string_enum_combo(ui: &mut egui::Ui, current: &str, options: &[StringOption]) -> Option<String> {
    let selected_label = options
        .iter()
        .find(|o| o.value == current)
        .map_or_else(|| current.to_string(), |o| o.name.clone());
    let mut chosen = current.to_string();
    let mut changed = false;
    egui::ComboBox::from_id_salt("str_enum")
        .width(ui.available_width())
        .selected_text(selected_label)
        .show_ui(ui, |ui| {
            for opt in options {
                if ui
                    .selectable_value(&mut chosen, opt.value.clone(), &opt.name)
                    .changed()
                {
                    changed = true;
                }
            }
        });
    changed.then_some(chosen)
}

/// Infers sensible decimal precision from a step size (`1.0` -> `Some(0)`,
/// `0.05` -> `Some(2)`), capped at 6 decimals.
fn step_decimals(step: f64) -> Option<usize> {
    let s = step.abs();
    if !s.is_finite() || s == 0.0 {
        return None;
    }
    let text = format!("{s}");
    if let Some((_, frac)) = text.split_once('.') {
        let trimmed = frac.trim_end_matches('0');
        Some(trimmed.len().min(6))
    } else {
        Some(0)
    }
}

/// Cleans IEEE-754 step-addition artefacts (such as `-0.25 + 11 * 0.05 =
/// 0.30000000000000004`) when a finite decimal step precision is known.
fn clean_step_float(val: f64, decimals: Option<usize>) -> f64 {
    let Some(dec) = decimals else {
        return val;
    };
    let factor = 10_f64.powi(dec as i32);
    let rounded = (val * factor).round() / factor;
    if rounded == 0.0 { 0.0 } else { rounded }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_precision_cleans_slider_artefacts() {
        assert_eq!(step_decimals(1.0), Some(0));
        assert_eq!(step_decimals(0.05), Some(2));
        assert_eq!(step_decimals(0.125), Some(3));
        assert_eq!(clean_step_float(-0.25 + 11.0 * 0.05, Some(2)), 0.3);
        assert_eq!(clean_step_float(-0.0, Some(2)), 0.0);
    }

    #[test]
    fn reload_preserves_compatible_overrides_and_apply_rewrites_file() {
        let dir = std::env::temp_dir().join(format!("app-customizer-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let scad_path = dir.join("box.scad");
        std::fs::write(
            &scad_path,
            "width = 50; // [10:1:100]\nmode = \"a\"; // [a, b]\ncube(width);\n",
        )
        .unwrap();

        let mut persisted = PersistedCustomizer::default();
        let mut state = CustomizerState::load_for_file(&scad_path, &persisted);
        assert_eq!(state.active_defines(), Vec::<(String, String)>::new());

        // Simulate user editing `width` to 80 and `mode` to `"b"`.
        state.values[0] = ParamValue::Number(80.0);
        state.values[1] = ParamValue::String("b".into());
        state.persist_into(&mut persisted);
        assert_eq!(
            state.active_defines(),
            vec![
                ("width".into(), "80".into()),
                ("mode".into(), "\"b\"".into()),
            ]
        );

        // External edit changes `mode` from string to boolean, while `width` stays numeric with tighter max 75.
        std::fs::write(
            &scad_path,
            "width = 50; // [10:1:75]\nmode = true;\ncube(width);\n",
        )
        .unwrap();
        state.reload_from_disk();
        assert_eq!(state.active_defines(), vec![("width".into(), "75".into())]);

        // Apply to file writes `75` into `box.scad` and clears active_defines.
        match state.apply_to_file() {
            CustomizerEvent::AppliedToFile { count, .. } => assert_eq!(count, 1),
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(!state.has_modifications());
        let on_disk = std::fs::read_to_string(&scad_path).unwrap();
        assert_eq!(
            on_disk,
            "width = 75; // [10:1:75]\nmode = true;\ncube(width);\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
