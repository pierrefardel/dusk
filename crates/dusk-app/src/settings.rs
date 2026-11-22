//! Fenêtre Réglages : Écrans, Écrans liés, Automatique, Raccourcis, Personnalisation, Avancé.

mod auto;
mod panel;
mod personal;

use crate::mediakeys;
use crate::i18n::{t, tx};
use crate::theme::{self, ns};
use dusk_core::{Config, Engine};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAppearanceCustomization, NSApplication,
    NSBackingStoreType, NSBox, NSButton, NSControlStateValueOff, NSControlStateValueOn, NSPopUpButton, NSScrollView,
    NSSlider, NSStackView, NSStackViewGravity, NSSwitch, NSTextField, NSView, NSWindow, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSTimer, NSURL};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

const WIDTH: f64 = 960.0;
const HEIGHT: f64 = 704.0;
const SIDEBAR: f64 = 208.0;
/// Largeur utile des pages (marges de 32 px).
const CONTENT: f64 = WIDTH - SIDEBAR - 2.0 * theme::S4;
/// Largeur utile dans une carte (marges de 16 px).
const CARD_INNER: f64 = CONTENT - 2.0 * theme::S2;
/// Pages de la barre latérale (clés du catalogue de textes).
const PAGES: [&str; 5] = ["nav.displays", "nav.auto", "nav.keys", "nav.personal", "nav.advanced"];
/// Pas proposés : (libellé, valeur en points).
const STEPS: [(&str, f32); 4] = [("step.16", 6.25), ("10 %", 10.0), ("5 %", 5.0), ("step.32", 3.125)];

pub struct Ivars {
    engine: Arc<Engine>,
    window: OnceCell<Retained<NSWindow>>,
    sidebar: OnceCell<Retained<NSStackView>>,
    sidebar_card: OnceCell<Retained<NSBox>>,
    content: OnceCell<Retained<NSBox>>,
    page: Cell<usize>,
    /// Écran affiché dans la page Écrans.
    display: Cell<usize>,
    /// « Plus de réglages » déplié.
    more: Cell<bool>,
    /// Libellés de valeur des curseurs, par étiquette de curseur.
    labels: RefCell<HashMap<isize, Retained<NSTextField>>>,
    scroll: RefCell<Option<Retained<NSScrollView>>>,
    /// État de chaque écran avant le premier changement depuis l'ouverture (clé : écran).
    before: RefCell<HashMap<String, dusk_core::DisplayState>>,
    undo: RefCell<Option<Retained<NSButton>>>,
    /// Page Automatique : courbe détaillée de « Suivre le soleil » affichée.
    solar_detail: Cell<bool>,
    /// Courbe détaillée, redessinée quand un curseur jour / nuit bouge.
    sun_view: RefCell<Option<Retained<crate::sun_view::SunView>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DuskSettings"]
    #[ivars = Ivars]
    pub struct Settings;

    unsafe impl NSObjectProtocol for Settings {}

    impl Settings {
        #[unsafe(method(selectPage:))]
        fn select_page(&self, sender: &NSButton) {
            self.ivars().page.set(sender.tag() as usize);
            self.ivars().solar_detail.set(false);
            self.render();
        }

        #[unsafe(method(selectDisplay:))]
        fn select_display(&self, sender: &NSButton) {
            self.ivars().display.set(sender.tag() as usize);
            self.render();
        }

        #[unsafe(method(pickTheme:))]
        fn pick_theme(&self, sender: &NSButton) {
            let id = theme::THEMES.get(sender.tag() as usize).copied().unwrap_or("minuit");
            self.ivars().engine.update_config(|c| c.theme = id.into());
            self.apply_look();
        }

        #[unsafe(method(pickAppearance:))]
        fn pick_appearance(&self, sender: &NSButton) {
            let a = personal::APPEARANCES.get(sender.tag() as usize).copied().unwrap_or("dark");
            self.ivars().engine.update_config(|c| c.appearance = a.into());
            self.apply_look();
        }

        #[unsafe(method(pickLanguage:))]
        fn pick_language(&self, sender: &NSPopUpButton) {
            let l = personal::LANGUAGES.get(sender.indexOfSelectedItem().max(0) as usize).copied().unwrap_or("auto");
            self.ivars().engine.update_config(|c| c.language = l.into());
            self.apply_look();
        }

        #[unsafe(method(toggleMore:))]
        fn toggle_more(&self, _sender: Option<&AnyObject>) {
            self.ivars().more.set(!self.ivars().more.get());
            self.render();
        }

        #[unsafe(method(panelBrightness:))]
        fn panel_brightness(&self, sender: &NSSlider) {
            self.remember(sender.tag() as usize / 256);
            let v = sender.doubleValue() as f32;
            self.ivars().engine.set_brightness(sender.tag() as usize / 256, v);
            self.set_value_label(sender.tag(), &crate::i18n::level(v));
        }

        #[unsafe(method(panelControl:))]
        fn panel_control(&self, sender: &NSSlider) {
            let tag = sender.tag() as usize;
            let Some(control) = dusk_core::Control::from_vcp(tag as u8) else { return };
            self.remember(tag / 256);
            let v = sender.doubleValue() as f32;
            self.ivars().engine.set_control(tag / 256, control, v);
            let text = match control {
                dusk_core::Control::Red | dusk_core::Control::Green | dusk_core::Control::Blue => format!("{v:.0}"),
                _ => panel::pct(v),
            };
            self.set_value_label(sender.tag(), &text);
        }

        #[unsafe(method(panelChoice:))]
        fn panel_choice(&self, sender: &NSButton) {
            let (index, code, value) = panel::decode_choice(sender.tag());
            if code == dusk_core::ddc::VCP_INPUT_SOURCE {
                let values = self.ivars().engine.displays().get(index).and_then(|d| d.panel.input.clone());
                let Some(input) = values else { return };
                if input.current == Some(value) {
                    return;
                }
                let name = crate::i18n::input(value, &input.values);
                let ok = self.confirm(&tx("input.confirm.title", &name), &tx("input.confirm.body", &name), t("input.confirm.ok"));
                if !ok {
                    return;
                }
            }
            self.remember(index);
            self.ivars().engine.set_choice(index, code, value);
            self.render();
        }

        #[unsafe(method(panelLanguage:))]
        fn panel_language(&self, sender: &NSPopUpButton) {
            let Some(item) = sender.selectedItem() else { return };
            let index = sender.tag() as usize;
            self.remember(index);
            self.ivars().engine.set_choice(index, dusk_core::panel::VCP_OSD_LANGUAGE, item.tag() as u8);
        }

        #[unsafe(method(panelMute:))]
        fn panel_mute(&self, sender: &NSButton) {
            let index = sender.tag() as usize;
            let engine = &self.ivars().engine;
            let muted = engine.displays().get(index).is_some_and(|d| d.muted);
            self.remember(index);
            engine.set_muted(index, !muted);
            self.render();
        }

        #[unsafe(method(panelSleep:))]
        fn panel_sleep(&self, sender: &NSButton) {
            if self.ivars().engine.sleep_display(sender.tag() as usize) {
                self.ivars().display.set(0);
                self.rescan_soon();
            }
        }

        /// Remet l'écran comme avant le premier changement depuis l'ouverture des Réglages.
        #[unsafe(method(undoChanges:))]
        fn undo_changes(&self, sender: &NSButton) {
            let index = sender.tag() as usize;
            let engine = &self.ivars().engine;
            let Some(now) = engine.displays().into_iter().nth(index) else { return };
            let Some(before) = self.ivars().before.borrow_mut().remove(&now.key) else { return };
            engine.set_brightness(index, before.brightness);
            // Préréglage de couleur avant le RVB : l'écran n'accepte le RVB qu'en « sur mesure ».
            for c in [&before.panel.color_preset, &before.panel.picture_mode, &before.panel.osd_language].into_iter().flatten() {
                if let Some(v) = c.current {
                    engine.set_choice(index, c.code, v);
                }
            }
            for (&code, &v) in &before.levels {
                if let Some(control) = dusk_core::Control::from_vcp(code) {
                    engine.set_control(index, control, v);
                }
            }
            if before.panel.mute && before.muted != now.muted {
                engine.set_muted(index, before.muted);
            }
            self.render();
        }

        #[unsafe(method(wakeDisplay:))]
        fn wake_display(&self, sender: &NSButton) {
            self.ivars().engine.wake_display(sender.tag() as u32);
            self.rescan_soon();
        }

        #[unsafe(method(panelReset:))]
        fn panel_reset(&self, sender: &NSButton) {
            let index = sender.tag() as usize;
            let engine = &self.ivars().engine;
            let Some(d) = engine.displays().into_iter().nth(index) else { return };
            if !self.confirm(&tx("reset.confirm.title", &d.name), t("reset.confirm.body"), t("reset.confirm.ok")) {
                return;
            }
            engine.factory_reset(index);
            self.render();
            // L'écran met quelques secondes à revenir : on relit alors ses réglages.
            if !d.write_only {
                self.rescan_soon();
            }
        }

        #[unsafe(method(rescanLater:))]
        fn rescan_later(&self, _t: &NSTimer) {
            self.ivars().engine.rescan();
            self.render();
        }

        /// Version gratuite : l'achat n'existe pas encore.

        #[unsafe(method(toggleRoom:))]
        fn toggle_room(&self, sender: &NSSwitch) {
            self.ivars().engine.set_room_enabled(sender.state() == NSControlStateValueOn);
            self.ivars().engine.solar_tick();
            self.render();
        }

        #[unsafe(method(toggleSolar:))]
        fn toggle_solar(&self, sender: &NSSwitch) {
            self.ivars().engine.set_solar_enabled(sender.state() == NSControlStateValueOn);
            self.ivars().engine.solar_tick();
            self.render();
        }

        #[unsafe(method(solarDetail:))]
        fn solar_detail(&self, _sender: Option<&AnyObject>) {
            self.ivars().solar_detail.set(true);
            self.render();
        }

        #[unsafe(method(solarBack:))]
        fn solar_back(&self, _sender: Option<&AnyObject>) {
            self.ivars().solar_detail.set(false);
            self.render();
        }

        #[unsafe(method(solarSpeed:))]
        fn solar_speed(&self, sender: &NSButton) {
            let minutes = auto::SPEEDS.get(sender.tag() as usize).map_or(40.0, |s| s.1);
            self.ivars().engine.set_solar_transition(minutes);
            self.render();
        }

        #[unsafe(method(solarLevel:))]
        fn solar_level(&self, sender: &NSSlider) {
            let engine = &self.ivars().engine;
            let (display, day) = auto::decode_level_tag(sender.tag());
            let key = match display {
                Some(i) => match engine.displays().into_iter().nth(i) {
                    Some(d) => d.key,
                    None => return,
                },
                None => String::new(),
            };
            let v = sender.doubleValue() as f32;
            engine.set_solar_level(&key, day, v);
            auto::set_tag_label(self, sender.tag(), v);
            self.redraw_sun();
        }

        #[unsafe(method(solarExclude:))]
        fn solar_exclude(&self, sender: &NSButton) {
            let Some(d) = self.ivars().engine.displays().into_iter().nth(sender.tag() as usize) else { return };
            self.ivars().engine.set_solar_excluded(&d.key, sender.state() != NSControlStateValueOn);
            self.render();
        }

        #[unsafe(method(toggleExtras:))]
        fn toggle_extras(&self, sender: &NSButton) {
            let on = sender.state() == NSControlStateValueOn;
            self.ivars().engine.set_extras_enabled(sender.tag() as usize, on);
        }

        #[unsafe(method(toggleReapply:))]
        fn toggle_reapply(&self, sender: &NSButton) {
            let on = sender.state() == NSControlStateValueOn;
            self.ivars().engine.update_config(|c| c.reapply_on_wake = on);
        }

        #[unsafe(method(rescan:))]
        fn rescan(&self, _sender: Option<&AnyObject>) {
            self.ivars().engine.rescan();
            self.render();
        }

        #[unsafe(method(stepChanged:))]
        fn step_changed(&self, sender: &NSPopUpButton) {
            let i = sender.indexOfSelectedItem().max(0) as usize;
            let v = STEPS.get(i).map_or(6.25, |s| s.1);
            self.ivars().engine.update_config(|c| c.step = v);
        }

        #[unsafe(method(openAccessibility:))]
        fn open_accessibility(&self, _sender: Option<&AnyObject>) {
            mediakeys::is_trusted(true);
            open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
        }

        #[unsafe(method(toggleLogin:))]
        fn toggle_login(&self, sender: &NSSwitch) {
            let service = unsafe { SMAppService::mainAppService() };
            let r = if sender.state() == NSControlStateValueOn {
                unsafe { service.registerAndReturnError() }
            } else {
                unsafe { service.unregisterAndReturnError() }
            };
            if let Err(e) = r {
                eprintln!("[dusk] ouverture de session : {e:?}");
            }
            self.render();
        }

        #[unsafe(method(openConfig:))]
        fn open_config(&self, _sender: Option<&AnyObject>) {
            let path = Config::path();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
                let url = NSURL::fileURLWithPath(&ns(&dir.to_string_lossy()));
                NSWorkspace::sharedWorkspace().openURL(&url);
            }
        }
    }
);

impl Settings {
    pub fn new(mtm: MainThreadMarker, engine: Arc<Engine>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            engine,
            window: OnceCell::new(),
            sidebar: OnceCell::new(),
            sidebar_card: OnceCell::new(),
            content: OnceCell::new(),
            page: Cell::new(0),
            display: Cell::new(0),
            more: Cell::new(std::env::var_os("DUSK_PREVIEW_MORE").is_some()),
            labels: RefCell::new(HashMap::new()),
            scroll: RefCell::new(None),
            before: RefCell::new(HashMap::new()),
            undo: RefCell::new(None),
            solar_detail: Cell::new(std::env::var_os("DUSK_PREVIEW_MORE").is_some()),
            sun_view: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn target(&self) -> &AnyObject {
        self
    }

    /// Vue à capturer en aperçu ; `DUSK_SNAPSHOT_FULL` donne toute la page défilante.
    pub fn content_view(&self) -> Option<Retained<NSView>> {
        if std::env::var_os("DUSK_SNAPSHOT_FULL").is_some() {
            if let Some(doc) = self.ivars().scroll.borrow().as_ref().and_then(|s| s.documentView()) {
                return Some(doc);
            }
        }
        self.ivars().window.get().and_then(|w| w.contentView())
    }

    pub fn show_page(&self, page: usize) {
        self.ivars().page.set(page.min(PAGES.len() - 1));
        self.show();
    }

    /// Écran affiché dans la page Écrans.
    pub fn show_display(&self, index: usize) {
        self.ivars().display.set(index);
    }

    pub fn show(&self) {
        let mtm = self.mtm();
        // Nouvelle visite : « Annuler mes changements » repart de l'état actuel.
        self.ivars().before.borrow_mut().clear();
        let window = self.ivars().window.get_or_init(|| self.build_window());
        self.render();
        window.center();
        window.makeKeyAndOrderFront(None);
        NSApplication::sharedApplication(mtm).activate();
    }

    fn build_window(&self) -> Retained<NSWindow> {
        let mtm = self.mtm();
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::FullSizeContentView;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT)),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&ns(t("window.title")));
        window.setTitlebarAppearsTransparent(true);
        window.setBackgroundColor(Some(&theme::window_bg()));

        let sidebar = theme::stack(mtm, true, theme::HALF);
        theme::loose_vertical(&sidebar);
        sidebar.setEdgeInsets(objc2_foundation::NSEdgeInsets { top: theme::S6, left: theme::S2, bottom: theme::S2, right: theme::S2 });
        let sidebar_card = theme::card(mtm, &sidebar, &theme::surface(), 0.0, 0.0);
        sidebar_card.widthAnchor().constraintEqualToConstant(SIDEBAR).setActive(true);

        let content = NSBox::new(mtm);
        content.setBoxType(objc2_app_kit::NSBoxType::Custom);
        content.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
        content.setBorderWidth(0.0);
        content.setFillColor(&theme::window_bg());
        content.setContentViewMargins(NSSize::new(0.0, 0.0));

        let root = theme::stack(mtm, false, 0.0);
        root.setAlignment(objc2_app_kit::NSLayoutAttribute::Top);
        root.addArrangedSubview(&sidebar_card);
        root.addArrangedSubview(&content);
        sidebar_card.heightAnchor().constraintEqualToAnchor(&root.heightAnchor()).setActive(true);
        content.heightAnchor().constraintEqualToAnchor(&root.heightAnchor()).setActive(true);
        window.setContentView(Some(&root));

        let _ = self.ivars().sidebar.set(sidebar);
        let _ = self.ivars().sidebar_card.set(sidebar_card);
        let _ = self.ivars().content.set(content);
        window
    }

    /// Reconstruit la barre latérale et la page courante.
    fn render(&self) {
        let mtm = self.mtm();
        let (Some(sidebar), Some(content)) = (self.ivars().sidebar.get(), self.ivars().content.get()) else {
            return;
        };
        for v in sidebar.arrangedSubviews().iter() {
            sidebar.removeArrangedSubview(&v);
            v.removeFromSuperview();
        }
        for v in sidebar.subviews().iter() {
            v.removeFromSuperview();
        }
        let current = self.ivars().page.get();
        // Thème, apparence ou langue ont pu changer : on repeint la fenêtre.
        if let Some(w) = self.ivars().window.get() {
            w.setAppearance(theme::appearance().as_deref());
            w.setBackgroundColor(Some(&theme::window_bg()));
            w.setTitle(&ns(t("window.title")));
        }
        if let Some(card) = self.ivars().sidebar_card.get() {
            card.setFillColor(&theme::surface());
        }
        content.setFillColor(&theme::window_bg());
        for (i, key) in PAGES.iter().enumerate() {
            let b = self.button(t(key), sel!(selectPage:));
            b.setTag(i as isize);
            b.setAlignment(objc2_app_kit::NSTextAlignment::Left);
            let selected = i == current;
            let tint = if i == current { theme::on_accent() } else { theme::text() };
            b.setContentTintColor(Some(&tint));
            let fill = if selected { theme::accent() } else { objc2_app_kit::NSColor::clearColor() };
            let item = theme::card(mtm, &b, &fill, theme::S1, theme::S1);
            theme::fixed_width(&item, SIDEBAR - 2.0 * theme::S2);
            theme::fixed_height(&item, theme::S4);
            sidebar.addView_inGravity(&item, NSStackViewGravity::Top);
        }
        let version = theme::label(mtm, &format!("Dusk {}", env!("CARGO_PKG_VERSION")), 11.0, false, &theme::faint());
        sidebar.addView_inGravity(&version, NSStackViewGravity::Bottom);

        let page = match current {
            0 => self.page_displays(),
            1 => self.page_auto(),
            2 => self.page_shortcuts(),
            3 => self.page_personal(),
            _ => self.page_advanced(),
        };
        let wrapper = theme::stack(mtm, true, theme::S3);
        wrapper.setEdgeInsets(objc2_foundation::NSEdgeInsets { top: theme::S6, left: theme::S4, bottom: theme::S4, right: theme::S4 });
        wrapper.addArrangedSubview(&page);

        // Défilement vertical : le panneau déplié dépasse la hauteur de la fenêtre.
        let offset = self.ivars().scroll.borrow().as_ref().map_or(0.0, |s| s.contentView().bounds().origin.y);
        let doc = theme::FlippedView::new(mtm);
        doc.addSubview(&wrapper);
        theme::pin(&doc, &wrapper, 0.0);
        let scroll = NSScrollView::new(mtm);
        scroll.setDrawsBackground(false);
        scroll.contentView().setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDocumentView(Some(&doc));
        doc.setTranslatesAutoresizingMaskIntoConstraints(false);
        let clip = scroll.contentView();
        doc.leadingAnchor().constraintEqualToAnchor(&clip.leadingAnchor()).setActive(true);
        doc.trailingAnchor().constraintEqualToAnchor(&clip.trailingAnchor()).setActive(true);
        doc.topAnchor().constraintEqualToAnchor(&clip.topAnchor()).setActive(true);
        content.setContentView(Some(&scroll));
        theme::pin(content, &scroll, 0.0);
        content.layoutSubtreeIfNeeded();
        if offset > 0.0 {
            doc.scrollPoint(NSPoint::new(0.0, offset));
        }
        *self.ivars().scroll.borrow_mut() = Some(scroll);
    }

    /// Applique thème, apparence et langue de la config, puis repeint.
    fn apply_look(&self) {
        let c = self.ivars().engine.config();
        theme::set(&c.theme, &c.appearance);
        crate::i18n::set_language(&c.language);
        self.render();
    }

    /// Repeint la fenêtre si elle est ouverte (ex. macOS passe en mode sombre).
    pub fn refresh(&self) {
        if self.ivars().window.get().is_some_and(|w| w.isVisible()) {
            self.render();
        }
    }

    /// Retient l'état de l'écran avant son premier changement, et active « Annuler mes changements ».
    fn remember(&self, index: usize) {
        let Some(d) = self.ivars().engine.displays().into_iter().nth(index) else { return };
        self.ivars().before.borrow_mut().entry(d.key.clone()).or_insert(d);
        if let Some(b) = self.ivars().undo.borrow().as_ref() {
            b.setEnabled(true);
        }
    }

    /// Redétection quelques secondes plus tard (écran qui bascule, s'éteint ou revient).
    fn rescan_soon(&self) {
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(3.0, self.target(), sel!(rescanLater:), None, false)
        };
    }

    fn set_value_label(&self, tag: isize, text: &str) {
        if let Some(l) = self.ivars().labels.borrow().get(&tag) {
            l.setStringValue(&ns(text));
        }
    }

    /// Demande de confirmation ; `true` si l'utilisateur valide.
    fn confirm(&self, title: &str, body: &str, ok: &str) -> bool {
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&ns(title));
        alert.setInformativeText(&ns(body));
        alert.addButtonWithTitle(&ns(ok));
        alert.addButtonWithTitle(&ns(t("cancel")));
        alert.runModal() == NSAlertFirstButtonReturn
    }

    // --- Pages --------------------------------------------------------------

    fn page_shortcuts(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let config = self.ivars().engine.config();
        let page = header(mtm, t("nav.keys"), t("keys.subtitle"));

        let trusted = mediakeys::is_trusted(false);
        let status = theme::label(
            mtm,
            if trusted { t("keys.ok") } else { t("keys.need") },
            13.0,
            true,
            &*if trusted { theme::ok() } else { theme::warn() },
        );
        let status_row = if trusted {
            theme::spread(mtm, &[&status], &[])
        } else {
            let b = self.bordered_button(t("keys.open"), sel!(openAccessibility:));
            theme::spread(mtm, &[&status], &[&b])
        };
        theme::fixed_width(&status_row, CARD_INNER);

        let body = theme::stack(mtm, true, theme::S1);
        body.addArrangedSubview(&status_row);
        body.setSpacing(theme::S2);
        let rows: [(&[&[&str]], &str); 4] = [
            (&[&["F1", "F2"]], t("keys.under")),
            (&[&["⌥"], &["F1", "F2"]], t("keys.all")),
            (&[&["⇧"], &["F1", "F2"]], t("keys.fine")),
            (&[&["⌃"], &["⌥"], &["⌘"], &["↑", "↓"]], t("keys.fallback")),
        ];
        for (keys, what) in rows {
            let combo = theme::kbd_combo(mtm, keys);
            let holder = theme::stack(mtm, false, 0.0);
            holder.addArrangedSubview(&combo);
            theme::fixed_width(&holder, 368.0);
            let row = theme::stack(mtm, false, theme::S2);
            row.addArrangedSubview(&holder);
            row.addArrangedSubview(&theme::label(mtm, what, 13.0, false, &theme::text()));
            body.addArrangedSubview(&row);
        }
        let card = theme::card(mtm, &body, &theme::card_bg(), theme::S2, theme::S2);
        theme::fixed_width(&card, CONTENT);
        page.addArrangedSubview(&card);

        let popup = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 26.0)),
            false,
        );
        for (label, _) in STEPS {
            popup.addItemWithTitle(&ns(if label.starts_with("step.") { t(label) } else { label }));
        }
        let selected = STEPS.iter().position(|s| (s.1 - config.step).abs() < 0.01).unwrap_or(0);
        popup.selectItemAtIndex(selected as isize);
        theme::fixed_width(&popup, 200.0);
        unsafe {
            popup.setTarget(Some(self.target()));
            popup.setAction(Some(sel!(stepChanged:)));
        }
        let step_label = theme::label(mtm, t("keys.step"), 13.0, false, &theme::text());
        let row = theme::spread(mtm, &[&step_label], &[&popup]);
        theme::fixed_width(&row, CONTENT);
        page.addArrangedSubview(&row);
        Retained::into_super(page)
    }

    fn page_advanced(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let page = header(mtm, t("nav.advanced"), t("adv.subtitle"));

        let enabled = unsafe { SMAppService::mainAppService().status() } == SMAppServiceStatus::Enabled;
        let switch = NSSwitch::new(mtm);
        switch.setState(if enabled { NSControlStateValueOn } else { NSControlStateValueOff });
        unsafe {
            switch.setTarget(Some(self.target()));
            switch.setAction(Some(sel!(toggleLogin:)));
        }
        let login = theme::label(mtm, t("adv.login"), 13.0, false, &theme::text());
        let row = theme::spread(mtm, &[&login], &[&switch]);
        theme::fixed_width(&row, CONTENT);
        page.addArrangedSubview(&row);

        let config = self.ivars().engine.config();
        page.addArrangedSubview(&self.checkbox(
            t("adv.reapply"),
            config.reapply_on_wake,
            sel!(toggleReapply:),
        ));
        let buttons = theme::stack(mtm, false, theme::S1);
        buttons.addArrangedSubview(&self.bordered_button(t("adv.rescan"), sel!(rescan:)));
        buttons.addArrangedSubview(&self.bordered_button(t("adv.config"), sel!(openConfig:)));
        page.addArrangedSubview(&buttons);
        Retained::into_super(page)
    }

    // --- Contrôles ----------------------------------------------------------

    fn button(&self, title: &str, action: Sel) -> Retained<NSButton> {
        let b = unsafe {
            NSButton::buttonWithTitle_target_action(&ns(title), Some(self.target()), Some(action), self.mtm())
        };
        b.setBordered(false);
        b
    }

    fn bordered_button(&self, title: &str, action: Sel) -> Retained<NSButton> {
        unsafe { NSButton::buttonWithTitle_target_action(&ns(title), Some(self.target()), Some(action), self.mtm()) }
    }

    fn checkbox(&self, title: &str, on: bool, action: Sel) -> Retained<NSButton> {
        let b = unsafe {
            NSButton::checkboxWithTitle_target_action(&ns(title), Some(self.target()), Some(action), self.mtm())
        };
        b.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
        b
    }
}

fn header(mtm: MainThreadMarker, title: &str, subtitle: &str) -> Retained<NSStackView> {
    let page = theme::stack(mtm, true, theme::S3);
    let head = theme::stack(mtm, true, theme::S1);
    head.addArrangedSubview(&theme::label(mtm, title, 20.0, true, &theme::text()));
    head.addArrangedSubview(&theme::wrapping(mtm, subtitle, 13.0, &theme::secondary(), CONTENT));
    page.addArrangedSubview(&head);
    page
}

/// Ligne « libellé : valeur ».
#[allow(dead_code)]
fn info(mtm: MainThreadMarker, key: &str, value: &str) -> Retained<NSStackView> {
    let k = theme::label(mtm, key, 12.0, false, &theme::secondary());
    theme::fixed_width(&k, 128.0);
    let v = theme::label(mtm, value, 12.0, false, &theme::text());
    let row = theme::stack(mtm, false, theme::S2);
    row.addArrangedSubview(&k);
    row.addArrangedSubview(&v);
    row
}

fn open_url(s: &str) {
    if let Some(url) = NSURL::URLWithString(&ns(s)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}
