//! HUD affiché lors d'un raccourci clavier : haut de l'écran sous le curseur,
//! disparaît après 1,2 s, ne prend jamais le focus.

use crate::theme;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearanceCustomization, NSBackingStoreType, NSColor,
    NSEvent, NSPanel, NSScreen, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::Cell;
use std::time::{Duration, Instant};

const WIDTH: f64 = 304.0;
/// Largeur utile dans la carte (marges de 24 px).
const INNER: f64 = WIDTH - 2.0 * theme::S3;
const VISIBLE: Duration = Duration::from_millis(1200);
const STATUS_WINDOW_LEVEL: isize = 25;
/// Distance entre le haut de la zone utile (sous la barre des menus) et le HUD.
const TOP_MARGIN: f64 = theme::S3;

pub struct Hud {
    panel: Retained<NSPanel>,
    hide_at: Cell<Option<Instant>>,
}

impl Hud {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, 80.0));
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(true);
        panel.setIgnoresMouseEvents(true);
        panel.setLevel(STATUS_WINDOW_LEVEL);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        Hud { panel, hide_at: Cell::new(None) }
    }

    /// `rows` : (nom, luminosité %). `title` : en-tête optionnel (mode synchro).
    pub fn show(&self, mtm: MainThreadMarker, title: Option<&str>, rows: &[(String, f32)]) {
        // Le thème a pu changer depuis le dernier affichage.
        self.panel.setAppearance(theme::appearance().as_deref());
        let body = theme::stack(mtm, true, theme::S2);
        if let Some(t) = title {
            body.addArrangedSubview(&theme::label(mtm, t, 12.0, true, &theme::text()));
        }
        for (name, v) in rows {
            let pct = theme::value_label(mtm, &crate::i18n::level(*v), 11.0);
            let line = theme::spread(mtm, &[&theme::label(mtm, name, 11.0, !title.is_some(), &theme::muted())], &[&pct]);
            theme::fixed_width(&line, INNER);
            // Même échelle que les curseurs (zone « Plus sombre » comprise).
            let min = dusk_core::DIM_MIN as f64;
            let bar = theme::pill_bar(mtm, (*v as f64 - min) / (100.0 - min), INNER, &theme::accent());
            let group = theme::stack(mtm, true, theme::S1);
            group.addArrangedSubview(&line);
            group.addArrangedSubview(&bar);
            body.addArrangedSubview(&group);
        }
        let card = theme::card(mtm, &body, &theme::surface(), theme::S3, theme::S3);
        self.panel.setContentView(Some(&card));
        let h = card.fittingSize().height.max(theme::S6 + theme::S2);

        // En haut de l'écran, sous la barre des menus.
        let frame = screen_under_mouse(mtm);
        let top = frame.origin.y + frame.size.height - TOP_MARGIN - h;
        let origin = NSPoint::new(frame.origin.x + (frame.size.width - WIDTH) / 2.0, top);
        self.panel.setFrame_display(NSRect::new(origin, NSSize::new(WIDTH, h)), true);
        self.panel.orderFrontRegardless();
        self.hide_at.set(Some(Instant::now() + VISIBLE));
    }

    pub fn tick(&self) {
        if self.hide_at.get().is_some_and(|t| Instant::now() >= t) {
            self.hide_at.set(None);
            self.panel.orderOut(None);
        }
    }
}

fn screen_under_mouse(mtm: MainThreadMarker) -> NSRect {
    let p = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    for s in screens.iter() {
        let f = s.frame();
        let vf = s.visibleFrame();
        if p.x >= f.origin.x && p.x < f.origin.x + f.size.width && p.y >= f.origin.y && p.y < f.origin.y + f.size.height {
            return vf;
        }
    }
    NSScreen::mainScreen(mtm).map(|s| s.visibleFrame()).unwrap_or(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)))
}
