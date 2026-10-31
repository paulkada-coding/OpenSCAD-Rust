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
    ui.heading("Schermo");
    ui.add_space(4.0);
    ui.label("Frequenza dei fotogrammi");
    ui.radio_value(
        &mut settings.frame_rate,
        FrameRate::Constant,
        "Costante (consigliata per monitor OLED e G-Sync/FreeSync)",
    );
    ui.radio_value(
        &mut settings.frame_rate,
        FrameRate::OnDemand,
        "Solo quando serve (minimo consumo)",
    );
    ui.add_space(4.0);
    ui.weak(
        "Con G-Sync/FreeSync la frequenza del monitor segue quella dell'app: se varia di \
         continuo, i pannelli OLED cambiano leggermente luminosità (flicker). In modalità \
         costante l'app ridisegna a ogni aggiornamento del monitor e la frequenza resta fissa.",
    );
    ui.add_space(4.0);
    ui.weak(
        "In alternativa si può disattivare G-Sync solo per questa app dal Pannello di \
         controllo NVIDIA → Gestisci impostazioni 3D → Impostazioni programma → \
         Tecnologia monitor: Frequenza di aggiornamento fissa.",
    );

    ui.add_space(8.0);
    let dt = ui.input(|i| i.stable_dt).max(1e-4);
    ui.label(format!("Fotogrammi al secondo: {:.0}", 1.0 / dt));
}
