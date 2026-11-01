//! User preferences, saved between runs by eframe's persistence.

use eframe::egui;
use serde::{Deserialize, Serialize};

pub const STORAGE_KEY: &str = "settings";

/// How often the window is redrawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameRate {
    /// Redraw every refresh. A steady frame rate keeps variable-refresh
    /// displays (G-Sync / FreeSync) at a fixed rate, which avoids the
    /// brightness flicker OLED panels show when the rate keeps changing.
    #[default]
    Constant,
    /// Redraw only when something changes: lowest power use.
    OnDemand,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub frame_rate: FrameRate,
}

impl Settings {
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        storage
            .and_then(|s| eframe::get_value(s, STORAGE_KEY))
            .unwrap_or_default()
    }
}

/// Contents of the settings window; changes apply immediately.
pub fn settings_ui(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Display");
    ui.add_space(4.0);
    ui.label("Frame rate");
    ui.radio_value(
        &mut settings.frame_rate,
        FrameRate::Constant,
        "Constant (recommended for OLED and G-Sync/FreeSync monitors)",
    );
    ui.radio_value(
        &mut settings.frame_rate,
        FrameRate::OnDemand,
        "On demand (lowest power usage)",
    );
    ui.add_space(4.0);
    ui.weak(
        "With G-Sync/FreeSync the monitor refresh rate follows the app: when it varies \
         continuously, OLED panels slightly shift brightness (VRR flicker). In constant mode \
         the app redraws on every monitor refresh, keeping the refresh rate steady.",
    );
    ui.add_space(4.0);
    ui.weak(
        "Alternatively, you can disable G-Sync for this app only in NVIDIA Control Panel → \
         Manage 3D settings → Program Settings → Monitor Technology: Fixed Refresh.",
    );

    ui.add_space(8.0);
    let dt = ui.input(|i| i.stable_dt).max(1e-4);
    ui.label(format!("Frames per second: {:.0}", 1.0 / dt));
}
