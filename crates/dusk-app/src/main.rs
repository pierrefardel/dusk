//! Dusk : app de barre des menus pour régler la luminosité des écrans au niveau matériel.

mod app;
mod hud;
mod i18n;
mod mediakeys;
mod settings;
mod sun_view;
mod theme;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    let mtm = MainThreadMarker::new().expect("Dusk doit démarrer sur le thread principal");
    let engine = dusk_core::Engine::new();
    let config = engine.config();
    // DUSK_THEME / DUSK_APPEARANCE : aperçus sans toucher à la config.
    let theme_id = std::env::var("DUSK_THEME").unwrap_or(config.theme.clone());
    let appearance = std::env::var("DUSK_APPEARANCE").unwrap_or(config.appearance.clone());
    theme::set(&theme_id, &appearance);
    i18n::set_language(&config.language);

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate = app::AppDelegate::new(mtm, engine);
    app.setDelegate(Some(app::delegate_object(&delegate)));
    app.run();
}
