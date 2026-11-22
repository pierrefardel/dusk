//! Délégué de l'application : icône de barre des menus, popover, raccourcis.

use crate::hud::Hud;
use crate::mediakeys::{self, KeyPress};
use crate::settings::Settings;
use crate::welcome::Welcome;
use crate::i18n::{t, tx};
use crate::theme::{self, ns};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use dusk_core::{Control, Engine};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::NSAccessibility;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication,
    NSApplicationDelegate, NSButton,
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventMask, NSImageView, NSSwitch, NSPopover, NSPopoverBehavior, NSSlider, NSStatusBar, NSStatusItem,
    NSTextField, NSVariableStatusItemLength, NSWorkspace, NSWorkspaceDidWakeNotification,
    NSApplicationDidChangeScreenParametersNotification, NSView, NSViewController,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSRectEdge, NSTimer};
use block2::RcBlock;
use std::cell::{Cell, OnceCell, RefCell};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const POPOVER_WIDTH: f64 = 352.0;
/// Largeur utile (marges de 16 px).
const INNER: f64 = POPOVER_WIDTH - 2.0 * theme::S2;
/// Largeur utile dans la carte maître (marges de 16 px).
const CARD_INNER: f64 = INNER - 2.0 * theme::S2;

/// Références vers les contrôles du popover, pour les mettre à jour sans le reconstruire.
struct PopoverRefs {
    master_slider: Retained<NSSlider>,
    master_label: Retained<NSTextField>,
    rows: Vec<Option<(Retained<NSSlider>, Retained<NSTextField>)>>,
    /// Contraste / volume : (écran, réglage, curseur, libellé).
    controls: Vec<(usize, Control, Retained<NSSlider>, Retained<NSTextField>)>,
    /// Phrase « Plus sombre » sous chaque écran, visible seulement sous zéro.
    notes: Vec<Option<Retained<NSTextField>>>,
}

struct Hotkeys {
    _manager: GlobalHotKeyManager,
    up: u32,
    down: u32,
}

pub struct Ivars {
    engine: Arc<Engine>,
    status_item: OnceCell<Retained<NSStatusItem>>,
    popover: OnceCell<Retained<NSPopover>>,
    controller: OnceCell<Retained<NSViewController>>,
    refs: RefCell<Option<PopoverRefs>>,
    hud: OnceCell<Hud>,
    hotkeys: OnceCell<Option<Hotkeys>>,
    timer: OnceCell<Retained<NSTimer>>,
    /// Touches luminosité interceptées (autorisation Accessibilité accordée).
    media_keys: Cell<bool>,
    ticks: Cell<u32>,
    settings: OnceCell<Retained<Settings>>,
    /// Redétection différée après un changement d'écrans (évite les rafales).
    rescan_timer: RefCell<Option<Retained<NSTimer>>>,
    /// Accueil du premier lancement (fenêtre de bienvenue).
    welcome: OnceCell<Retained<Welcome>>,
    /// Bulle « C'est prêt » à montrer : avec les touches (autorisées) ou sans.
    ready_keys: Cell<bool>,
    /// Relecture des écrans en cours (ouverture du popover) / valeurs relues à afficher.
    polling: Arc<AtomicBool>,
    polled: Arc<AtomicBool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DuskAppDelegate"]
    #[ivars = Ivars]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            self.setup();
        }
    }

    impl AppDelegate {
        #[unsafe(method(togglePopover:))]
        fn toggle_popover(&self, _sender: Option<&AnyObject>) {
            self.toggle();
        }

        #[unsafe(method(masterChanged:))]
        fn master_changed(&self, sender: &NSSlider) {
            self.ivars().engine.set_master(sender.doubleValue() as f32);
            self.refresh_values();
        }

        #[unsafe(method(displayChanged:))]
        fn display_changed(&self, sender: &NSSlider) {
            let index = sender.tag() as usize;
            self.ivars().engine.set_brightness(index, sender.doubleValue() as f32);
            self.refresh_values();
        }

        #[unsafe(method(controlChanged:))]
        fn control_changed(&self, sender: &NSSlider) {
            let tag = sender.tag() as usize;
            let control = if tag % 10 == 1 { Control::Contrast } else { Control::Volume };
            let v = sender.doubleValue() as f32;
            self.ivars().engine.set_control(tag / 10, control, v);
            if let Some(r) = self.ivars().refs.borrow().as_ref() {
                for (i, c, _, label) in &r.controls {
                    if *i == tag / 10 && *c == control {
                        label.setStringValue(&ns(&control_text(control, v)));
                    }
                }
            }
        }

        /// macOS change d'apparence : utile quand Dusk suit le système.
        #[unsafe(method(systemThemeChanged:))]
        fn system_theme_changed(&self, _n: &NSNotification) {
            if let Some(s) = self.ivars().settings.get() {
                s.refresh();
            }
        }

        #[unsafe(method(wakeDisplay:))]
        fn wake_display(&self, sender: &NSButton) {
            self.ivars().engine.wake_display(sender.tag() as u32);
            if let Some(p) = self.ivars().popover.get() {
                unsafe { p.performClose(None) };
            }
        }

        /// Interrupteur « Automatique » du popover.
        #[unsafe(method(autoToggled:))]
        fn auto_toggled(&self, sender: &NSSwitch) {
            let engine = &self.ivars().engine;
            engine.set_solar_enabled(sender.state() == NSControlStateValueOn);
            // Rien d'autre ne change : pas de reconstruction (elle laissait un artefact
            // de l'animation de l'interrupteur à l'écran).
            engine.solar_tick();
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            if let Some(p) = self.ivars().popover.get() {
                unsafe { p.performClose(None) };
            }
            let settings = self
                .ivars()
                .settings
                .get_or_init(|| Settings::new(self.mtm(), self.ivars().engine.clone()));
            settings.show();
        }

        /// Réveil ou changement d'écrans : on attend que les écrans soient prêts.
        #[unsafe(method(displaysChanged:))]
        fn displays_changed(&self, _n: &NSNotification) {
            if let Some(t) = self.ivars().rescan_timer.borrow_mut().take() {
                t.invalidate();
            }
            let timer = unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    3.0,
                    self.target(),
                    sel!(rescanNow:),
                    None,
                    false,
                )
            };
            *self.ivars().rescan_timer.borrow_mut() = Some(timer);
        }

        #[unsafe(method(rescanNow:))]
        fn rescan_now(&self, _t: &NSTimer) {
            self.ivars().rescan_timer.borrow_mut().take();
            let engine = &self.ivars().engine;
            engine.rescan();
            engine.solar_jump();
            if engine.config().reapply_on_wake {
                engine.reapply();
            }
            self.refresh_values();
        }

        #[unsafe(method(snapshot:))]
        fn snapshot(&self, _t: &NSTimer) {
            let Ok(path) = std::env::var("DUSK_SNAPSHOT") else { return };
            let preview = std::env::var("DUSK_PREVIEW").unwrap_or_default();
            if preview.starts_with("ready") {
                let view = self.build_ready(!preview.ends_with(":nokeys"));
                view.setFrameSize(view.fittingSize());
                view.layoutSubtreeIfNeeded();
                theme::snapshot(&view, &path);
            } else if let Some(view) = self.ivars().welcome.get().and_then(|w| w.content_view()) {
                theme::snapshot(&view, &path);
            } else if let Some(view) = self.ivars().settings.get().and_then(|s| s.content_view()) {
                theme::snapshot(&view, &path);
            } else {
                // Contenu du popover rendu hors écran (indépendant de son affichage).
                let view = self.build_content();
                view.setFrameSize(view.fittingSize());
                view.layoutSubtreeIfNeeded();
                theme::snapshot(&view, &path);
            }
        }

        /// Bulle « C'est prêt », accrochée à l'icône de la barre des menus.
        #[unsafe(method(showReady:))]
        fn show_ready_now(&self, _t: Option<&NSTimer>) {
            self.show_ready(self.ivars().ready_keys.get());
        }

        #[unsafe(method(readyDone:))]
        fn ready_done(&self, _sender: Option<&AnyObject>) {
            if let Some(p) = self.ivars().popover.get() {
                unsafe { p.performClose(None) };
            }
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            // Sans autorisation, on retente l'interception toutes les 2 s.
            let n = self.ivars().ticks.get().wrapping_add(1);
            self.ivars().ticks.set(n);
            if !self.ivars().media_keys.get() && n % 40 == 0 && mediakeys::start() {
                // Autorisation accordée pendant que Dusk tourne : l'interception
                // marche, mais macOS laisse aussi passer les touches (double réglage).
                // Un redémarrage règle ça, Dusk le fait donc tout seul.
                if relaunch() {
                    NSApplication::sharedApplication(self.mtm()).terminate(None);
                    return;
                }
                self.ivars().media_keys.set(true);
                if let Some(w) = self.ivars().welcome.get().filter(|w| w.is_open()) {
                    w.granted();
                }
            }
            if let Some(w) = self.ivars().welcome.get() {
                w.tick();
            }
            self.poll_hotkeys();
            // Valeurs relues sur les écrans à l'ouverture du popover.
            if self.ivars().polled.swap(false, Ordering::Relaxed) {
                self.refresh_values();
            }
            if let Some(h) = self.ivars().hud.get() {
                h.tick();
            }
            // Lumière de la pièce : une mesure toutes les 2 s (rien sans capteur).
            if n % 40 == 0 && self.ivars().engine.ambient_tick() {
                self.refresh_values();
            }
            // Suivre le soleil : un pas toutes les 30 s.
            if n % 600 == 0 && self.ivars().engine.solar_tick() {
                self.refresh_values();
            }
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker, engine: Arc<Engine>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            engine,
            status_item: OnceCell::new(),
            popover: OnceCell::new(),
            controller: OnceCell::new(),
            refs: RefCell::new(None),
            hud: OnceCell::new(),
            hotkeys: OnceCell::new(),
            timer: OnceCell::new(),
            media_keys: Cell::new(false),
            ticks: Cell::new(0),
            settings: OnceCell::new(),
            rescan_timer: RefCell::new(None),
            welcome: OnceCell::new(),
            ready_keys: Cell::new(false),
            polling: Arc::new(AtomicBool::new(false)),
            polled: Arc::new(AtomicBool::new(false)),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn target(&self) -> &AnyObject {
        self
    }

    fn setup(&self) {
        let mtm = self.mtm();

        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = item.button(mtm) {
            button.setImage(Some(&theme::wand_image()));
            unsafe {
                button.setTarget(Some(self.target()));
                button.setAction(Some(sel!(togglePopover:)));
            }
        }
        let _ = self.ivars().status_item.set(item);

        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setAnimates(true);
        let controller = NSViewController::new(mtm);
        popover.setContentViewController(Some(&controller));
        // Dusk n'est pas toujours l'app active : le mode transitoire ne voit alors pas
        // les clics faits ailleurs. On les surveille pour fermer le popover comme attendu.
        let target = popover.clone();
        let on_click = RcBlock::new(move |_: NonNull<NSEvent>| {
            if target.isShown() {
                unsafe { target.performClose(None) };
            }
        });
        let mask = NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
        // Le moniteur vit autant que l'app : pas besoin de le retirer.
        std::mem::forget(NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &on_click));
        let _ = self.ivars().popover.set(popover);
        let _ = self.ivars().controller.set(controller);

        let _ = self.ivars().hud.set(Hud::new(mtm));
        let _ = self.ivars().hotkeys.set(register_hotkeys());
        // L'autorisation des touches se demande depuis l'accueil ou Réglages › Raccourcis,
        // jamais d'office à chaque lancement.
        let trusted = mediakeys::start();
        self.ivars().media_keys.set(trusted);

        // Réveil et branchement / débranchement d'écrans.
        unsafe {
            NSWorkspace::sharedWorkspace().notificationCenter().addObserver_selector_name_object(
                self.target(),
                sel!(displaysChanged:),
                Some(NSWorkspaceDidWakeNotification),
                None,
            );
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self.target(),
                sel!(displaysChanged:),
                Some(NSApplicationDidChangeScreenParametersNotification),
                None,
            );
        }

        unsafe {
            objc2_foundation::NSDistributedNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self.target(),
                sel!(systemThemeChanged:),
                Some(&ns("AppleInterfaceThemeChangedNotification")),
                None,
            );
        }

        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                0.05,
                self.target(),
                sel!(tick:),
                None,
                true,
            )
        };
        let _ = self.ivars().timer.set(timer);
        self.ivars().engine.solar_jump();

        let preview = std::env::var("DUSK_PREVIEW").ok();
        if preview.is_none() {
            match self.ivars().engine.config().welcome {
                // Relancé après l'autorisation : la bulle « C'est prêt ».
                dusk_core::Welcome::Waiting if trusted => {
                    self.ivars().engine.update_config(|c| c.welcome = dusk_core::Welcome::Done);
                    self.schedule_ready(true);
                }
                dusk_core::Welcome::Done => {}
                _ => self.welcome().show(0),
            }
        }

        // Aperçu pour le développement : DUSK_PREVIEW=popover | hud | settings[:page[:écran]]
        // | welcome[:étape] | ready[:nokeys].
        if let Some(p) = preview {
            match p.split(':').next() {
                Some("popover") => self.toggle(),
                Some("welcome") => {
                    let step = p.split(':').nth(1).and_then(|n| n.parse().ok()).unwrap_or(0);
                    self.welcome().show(step);
                }
                Some("ready") => self.show_ready(!p.ends_with(":nokeys")),
                Some("hud") => self.show_hud(None),
                Some("settings") => {
                    let page = p.split(':').nth(1).and_then(|n| n.parse().ok()).unwrap_or(0);
                    let display = p.split(':').nth(2).and_then(|n| n.parse().ok()).unwrap_or(0);
                    let s = self.ivars().settings.get_or_init(|| Settings::new(mtm, self.ivars().engine.clone()));
                    s.show_display(display);
                    s.show_page(page);
                }
                _ => {}
            }
            if std::env::var_os("DUSK_SNAPSHOT").is_some() {
                unsafe {
                    NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                        1.5,
                        self.target(),
                        sel!(snapshot:),
                        None,
                        false,
                    )
                };
            }
        }
    }

    fn welcome(&self) -> &Welcome {
        self.ivars().welcome.get_or_init(|| {
            let this = objc2::Message::retain(self);
            let on_finish = Box::new(move |keys: bool| this.schedule_ready(keys));
            Welcome::new(self.mtm(), self.ivars().engine.clone(), on_finish)
        })
    }

    /// Bulle « C'est prêt » un instant plus tard (icône de barre des menus en place).
    fn schedule_ready(&self, keys: bool) {
        self.ivars().ready_keys.set(keys);
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                0.5,
                self.target(),
                sel!(showReady:),
                None,
                false,
            )
        };
    }

    fn show_ready(&self, keys: bool) {
        let mtm = self.mtm();
        let (Some(popover), Some(item)) = (self.ivars().popover.get(), self.ivars().status_item.get()) else {
            return;
        };
        popover.setAppearance(theme::appearance().as_deref());
        let content = self.build_ready(keys);
        if let Some(c) = self.ivars().controller.get() {
            c.setView(&content);
        }
        self.resize_popover();
        if let Some(button) = item.button(mtm) {
            popover.showRelativeToRect_ofView_preferredEdge(button.bounds(), &button, NSRectEdge::MinY);
        }
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }

    /// Contenu de la bulle : où vit Dusk et, touches autorisées, les deux gestes à retenir.
    fn build_ready(&self, keys: bool) -> Retained<NSView> {
        let mtm = self.mtm();
        let inner = POPOVER_WIDTH - 2.0 * theme::S3;
        let card_inner = inner - 2.0 * theme::S2;
        let root = theme::stack(mtm, true, theme::S2);
        theme::insets(&root, theme::S3, theme::S3);
        theme::fixed_width(&root, POPOVER_WIDTH);

        let head = theme::stack(mtm, true, theme::HALF);
        head.addArrangedSubview(&theme::label(mtm, t("ready.title"), 20.0, true, &theme::text()));
        head.addArrangedSubview(&theme::wrapping(mtm, t("ready.lead"), 13.0, &theme::secondary(), inner));
        root.addArrangedSubview(&head);

        if keys {
            let rows = theme::stack(mtm, true, theme::S2);
            let combos: [(&[&[&str]], &str); 2] =
                [(&[&["F1", "F2"]], t("ready.under")), (&[&["⌥"], &["F1", "F2"]], t("keys.all"))];
            for (groups, text) in combos {
                let keys = theme::kbd_combo(mtm, groups);
                let label = theme::label(mtm, text, 12.0, false, &theme::secondary());
                let row = theme::spread(mtm, &[&keys], &[&label]);
                theme::fixed_width(&row, card_inner);
                rows.addArrangedSubview(&row);
            }
            let card = theme::card(mtm, &rows, &theme::card_bg(), theme::S1 + theme::HALF, theme::S2);
            theme::fixed_width(&card, inner);
            root.addArrangedSubview(&card);
        }

        let b = self.button(t("ready.done"), Some(sel!(readyDone:)));
        let done = theme::pill_button(mtm, &b, theme::Pill::Primary);
        let row = theme::spread(mtm, &[], &[&done]);
        theme::fixed_width(&row, inner);
        root.addArrangedSubview(&row);

        let bg = theme::card(mtm, &root, &theme::surface(), 0.0, 0.0);
        Retained::into_super(bg)
    }

    fn toggle(&self) {
        let mtm = self.mtm();
        let (Some(popover), Some(item)) = (self.ivars().popover.get(), self.ivars().status_item.get()) else {
            return;
        };
        if popover.isShown() {
            unsafe { popover.performClose(None) };
            return;
        }
        // Reconstruit le contenu à chaque ouverture : valeurs, écrans et thème à jour.
        popover.setAppearance(theme::appearance().as_deref());
        let content = self.build_content();
        if let Some(c) = self.ivars().controller.get() {
            c.setView(&content);
        }
        self.resize_popover();
        if let Some(button) = item.button(mtm) {
            popover.showRelativeToRect_ofView_preferredEdge(button.bounds(), &button, NSRectEdge::MinY);
        }
        // `activate()` n'est qu'une demande que macOS peut ignorer : le popover s'affichait
        // alors comme une fenêtre inactive (interrupteur et curseurs gris). On force donc.
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        if let Some(w) = self.ivars().controller.get().and_then(|c| c.view().window()) {
            w.makeKeyWindow();
        }
        self.poll_displays();
    }

    /// Relit les écrans en arrière-plan (boutons de l'écran, réglage fait ailleurs) ;
    /// les curseurs suivent au tic suivant si une valeur a changé.
    fn poll_displays(&self) {
        if self.ivars().polling.swap(true, Ordering::Relaxed) {
            return;
        }
        let (engine, polling, polled) =
            (self.ivars().engine.clone(), self.ivars().polling.clone(), self.ivars().polled.clone());
        let spawned = std::thread::Builder::new().name("dusk-poll".into()).spawn(move || {
            if engine.poll_hardware() {
                polled.store(true, Ordering::Relaxed);
            }
            polling.store(false, Ordering::Relaxed);
        });
        if spawned.is_err() {
            self.ivars().polling.store(false, Ordering::Relaxed);
        }
    }

    fn resize_popover(&self) {
        let (Some(popover), Some(c)) = (self.ivars().popover.get(), self.ivars().controller.get()) else {
            return;
        };
        let view = c.view();
        view.layoutSubtreeIfNeeded();
        popover.setContentSize(view.fittingSize());
    }

    fn build_content(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let engine = &self.ivars().engine;
        let displays = engine.displays();
        // « Tous les écrans » dès deux écrans : pas de mode à activer.
        let several = displays.len() > 1;

        let root = theme::stack(mtm, true, theme::S2);
        theme::insets(&root, theme::S2, theme::S2);
        theme::fixed_width(&root, POPOVER_WIDTH);

        // En-tête : baguette + nom, interrupteur de synchro.
        let wand = NSImageView::imageViewWithImage(&theme::wand_image(), mtm);
        wand.setContentTintColor(Some(&theme::spark()));
        let title = theme::label(mtm, greeting(), 17.0, true, &theme::text());
        // Lier ou délier les écrans se fait dans Réglages › Écrans liés.
        let header = theme::spread(mtm, &[&wand, &title], &[]);
        theme::fixed_width(&header, INNER);
        root.addArrangedSubview(&header);

        // Carte maître.
        let master = engine.master();
        let master_label = theme::value_label(mtm, &crate::i18n::level(master), 12.0);
        let master_title = theme::label(mtm, t("pop.all"), 12.0, true, &theme::text());
        let master_head = theme::spread(mtm, &[&master_title], &[&master_label]);
        theme::fixed_width(&master_head, CARD_INNER);
        let master_slider = self.slider(master, -1, sel!(masterChanged:), &theme::accent());
        theme::dim_range(&master_slider);
        let slider_row = theme::stack(mtm, false, theme::S1);
        {
            let i = theme::icon(theme::Icon::SunDim, 16.0);
            let v = NSImageView::imageViewWithImage(&i, mtm);
            v.setContentTintColor(Some(&theme::secondary()));
            slider_row.addArrangedSubview(&v);
        }
        slider_row.addArrangedSubview(&master_slider);
        {
            let i = theme::icon(theme::Icon::Sun, 16.0);
            let v = NSImageView::imageViewWithImage(&i, mtm);
            v.setContentTintColor(Some(&theme::text()));
            slider_row.addArrangedSubview(&v);
        }
        theme::fixed_width(&slider_row, CARD_INNER);
        let master_body = theme::stack(mtm, true, theme::S1);
        master_body.addArrangedSubview(&master_head);
        master_body.addArrangedSubview(&slider_row);
        let master_card = theme::card(mtm, &master_body, &theme::card_bg(), theme::S2, theme::S2);
        theme::fixed_width(&master_card, INNER);
        master_card.setHidden(!several);
        root.addArrangedSubview(&master_card);

        // Réglage automatique (suivre le soleil).
        if let Some(line) = self.solar_line() {
            root.addArrangedSubview(&line);
        }

        // Écrans.
        root.addArrangedSubview(&theme::label(mtm, t("pop.displays"), 11.0, true, &theme::secondary()));
        let mut rows = Vec::new();
        let mut notes = Vec::new();
        let mut controls = Vec::new();
        for (i, d) in displays.iter().enumerate() {
            if i > 0 {
                let sep = theme::separator(mtm);
                theme::fixed_width(&sep, INNER);
                root.addArrangedSubview(&sep);
            }
            let dot = theme::label(mtm, "●", 10.0, false, &theme::display_color(i));
            let name = theme::label(mtm, &d.name, 13.0, true, &theme::text());
            let value = theme::value_label(mtm, &crate::i18n::level(d.brightness), 12.0);
            let head = theme::spread(mtm, &[&dot, &name], &[&value]);
            theme::fixed_width(&head, INNER);
            let group = theme::stack(mtm, true, theme::S1);
            group.addArrangedSubview(&head);
            {
                let s = self.slider(d.brightness, i as isize, sel!(displayChanged:), &theme::display_color(i));
                if !d.software {
                    theme::dim_range(&s);
                }
                theme::fixed_width(&s, INNER);
                group.addArrangedSubview(&s);
                rows.push(Some((s, value)));
                let note = theme::wrapping(mtm, t("dim.hint"), 11.0, &theme::secondary(), INNER);
                note.setHidden(d.brightness >= 0.0);
                group.addArrangedSubview(&note);
                notes.push(Some(note));
                // Le popover reste simple : luminosité et son. Le contraste vit dans Réglages.
                let extras: Vec<(Control, f32)> = d.volume.map(|v| (Control::Volume, v)).into_iter().collect();
                if !extras.is_empty() {
                    let grid = theme::stack(mtm, false, theme::S2);
                    let col = INNER;
                    for (c, v) in extras {
                        let label = theme::label(mtm, &control_text(c, v), 11.0, false, &theme::secondary());
                        let tag = (i * 10 + if c == Control::Contrast { 1 } else { 2 }) as isize;
                        let s = self.slider(v, tag, sel!(controlChanged:), &theme::secondary());
                        s.setControlSize(objc2_app_kit::NSControlSize::Small);
                        theme::fixed_width(&s, col);
                        let cell = theme::stack(mtm, true, theme::HALF);
                        cell.addArrangedSubview(&label);
                        cell.addArrangedSubview(&s);
                        grid.addArrangedSubview(&cell);
                        controls.push((i, c, s, label));
                    }
                    group.addArrangedSubview(&grid);
                }
            }
            root.addArrangedSubview(&group);
        }

        // Écrans éteints depuis Dusk : un bouton pour les rallumer.
        for s in engine.sleeping() {
            let name = theme::label(mtm, &tx("sleeping.title", &s.name), 12.0, false, &theme::secondary());
            let wake = self.button(t("wake.btn"), Some(sel!(wakeDisplay:)));
            wake.setTag(s.id as isize);
            wake.setContentTintColor(Some(&theme::muted()));
            let row = theme::spread(mtm, &[&name], &[&wake]);
            theme::fixed_width(&row, INNER);
            root.addArrangedSubview(&row);
        }

        let sep = theme::separator(mtm);
        theme::fixed_width(&sep, INNER);
        root.addArrangedSubview(&sep);

        // Pied : deux icônes, leur nom en infobulle (et pour VoiceOver).
        let settings = self.icon_button(theme::Icon::Settings, t("pop.settings"), sel!(openSettings:));
        let quit = self.icon_button(theme::Icon::Power, t("pop.quit"), sel!(quit:));
        let footer = theme::spread(mtm, &[&settings], &[&quit]);
        theme::fixed_width(&footer, INNER);
        root.addArrangedSubview(&footer);

        *self.ivars().refs.borrow_mut() = Some(PopoverRefs { master_slider, master_label, rows, controls, notes });
        // Fond indigo sur tout le popover (le style F1 ne suit pas le gris système).
        let bg = theme::card(mtm, &root, &theme::surface(), 0.0, 0.0);
        Retained::into_super(bg)
    }

    /// Ligne « Suit le soleil… » avec « Pause 1 h » / « Reprendre ».
    /// Réglage automatique : un interrupteur et une phrase simple, sans heures ni jargon.
    fn solar_line(&self) -> Option<Retained<NSView>> {
        let mtm = self.mtm();
        let st = self.ivars().engine.solar_status();
        let room = st.room_available && st.room_enabled;
        if st.sun.is_none() && !room {
            return None;
        }
        let switch = NSSwitch::new(mtm);
        switch.setControlSize(objc2_app_kit::NSControlSize::Small);
        switch.setState(if st.enabled { NSControlStateValueOn } else { NSControlStateValueOff });
        unsafe {
            switch.setTarget(Some(self.target()));
            switch.setAction(Some(sel!(autoToggled:)));
        }
        let width = CARD_INNER - switch.fittingSize().width - theme::S2;
        let text = theme::stack(mtm, true, theme::HALF / 2.0);
        text.addArrangedSubview(&theme::label(mtm, t("pop.auto.title"), 12.0, true, &theme::text()));
        let desc = if room { t("pop.auto.room") } else { t("pop.auto.desc") };
        text.addArrangedSubview(&theme::wrapping(mtm, desc, 11.0, &theme::secondary(), width));
        let line = theme::spread(mtm, &[&text], &[&switch]);
        theme::fixed_width(&line, CARD_INNER);
        let card = theme::card(mtm, &line, &theme::card_bg(), theme::S1 * 1.5, theme::S2);
        theme::fixed_width(&card, INNER);
        Some(Retained::into_super(card))
    }

    fn slider(&self, value: f32, tag: isize, action: objc2::runtime::Sel, fill: &objc2_app_kit::NSColor) -> Retained<NSSlider> {
        let s = unsafe {
            NSSlider::sliderWithValue_minValue_maxValue_target_action(
                value as f64,
                0.0,
                100.0,
                Some(self.target()),
                Some(action),
                self.mtm(),
            )
        };
        s.setTag(tag);
        s.setContinuous(true);
        s.setTrackFillColor(Some(fill));
        s
    }

    fn icon_button(&self, icon: theme::Icon, name: &str, action: objc2::runtime::Sel) -> Retained<NSButton> {
        let b = unsafe {
            NSButton::buttonWithImage_target_action(&theme::icon(icon, 18.0), Some(self.target()), Some(action), self.mtm())
        };
        b.setBordered(false);
        b.setContentTintColor(Some(&theme::secondary()));
        b.setToolTip(Some(&ns(name)));
        theme::fixed_width(&b, theme::S4);
        theme::fixed_height(&b, theme::S4);
        b.setAccessibilityLabel(Some(&ns(name)));
        b
    }

    fn button(&self, title: &str, action: Option<objc2::runtime::Sel>) -> Retained<NSButton> {
        let b = unsafe {
            NSButton::buttonWithTitle_target_action(&ns(title), Some(self.target()), action, self.mtm())
        };
        b.setBordered(false);
        b
    }

    /// Met à jour sliders et libellés depuis le moteur (après un réglage ou un raccourci).
    fn refresh_values(&self) {
        let engine = &self.ivars().engine;
        let refs = self.ivars().refs.borrow();
        let Some(r) = refs.as_ref() else { return };
        let master = engine.master();
        r.master_slider.setDoubleValue(master as f64);
        r.master_label.setStringValue(&ns(&crate::i18n::level(master)));
        let mut relayout = false;
        for ((row, note), d) in r.rows.iter().zip(&r.notes).zip(engine.displays()) {
            if let Some((s, l)) = row {
                s.setDoubleValue(d.brightness as f64);
                l.setStringValue(&ns(&crate::i18n::level(d.brightness)));
            }
            if let Some(n) = note {
                let hide = d.brightness >= 0.0;
                if n.isHidden() != hide {
                    n.setHidden(hide);
                    relayout = true;
                }
            }
        }
        let displays = engine.displays();
        for (i, c, s, l) in &r.controls {
            if let Some(v) = displays.get(*i).and_then(|d| d.level(*c)) {
                s.setDoubleValue(v as f64);
                l.setStringValue(&ns(&control_text(*c, v)));
            }
        }
        drop(refs);
        if relayout {
            self.resize_popover();
        }
    }

    fn poll_hotkeys(&self) {
        let mut presses = mediakeys::drain();
        if let Some(Some(hk)) = self.ivars().hotkeys.get() {
            while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
                if ev.state != HotKeyState::Pressed {
                    continue;
                }
                if ev.id == hk.up || ev.id == hk.down {
                    presses.push(KeyPress { up: ev.id == hk.up, fine: false, all: false });
                }
            }
        }
        for p in presses {
            self.handle_press(p);
        }
    }

    /// Touche + / − : écran sous le curseur ; ⌥ = tous les écrans ; ⇧ = pas fin (1 %).
    fn handle_press(&self, p: KeyPress) {
        let engine = &self.ivars().engine;
        let size = if p.fine { 1.0 } else { engine.step_size() };
        let delta = if p.up { size } else { -size };
        let single = if p.all {
            None
        } else {
            dusk_core::display_under_cursor().and_then(|id| engine.step_display(id, delta))
        };
        if single.is_none() {
            engine.step_all(delta);
        }
        self.refresh_values();
        self.show_hud(single);
    }

    fn show_hud(&self, single: Option<usize>) {
        let engine = &self.ivars().engine;
        let displays = engine.displays();
        let (title, rows): (Option<String>, Vec<(String, f32)>) = match single {
            Some(i) => (None, vec![(displays[i].name.clone(), displays[i].brightness)]),
            None => (
                (displays.len() > 1).then(|| tx("hud.all", &crate::i18n::level(engine.master()))),
                displays.into_iter().map(|d| (d.name, d.brightness)).collect(),
            ),
        };
        if let Some(h) = self.ivars().hud.get() {
            h.show(self.mtm(), title.as_deref(), &rows);
        }
    }
}

/// Salutation selon l'heure locale.
fn greeting() -> &'static str {
    let hour = unsafe {
        let mut t: libc::time_t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&mut t, &mut tm);
        tm.tm_hour
    };
    t(if (5..18).contains(&hour) { "pop.hello" } else { "pop.evening" })
}

fn control_text(c: Control, v: f32) -> String {
    let name = match c {
        Control::Contrast => t("contrast"),
        Control::Volume => t("volume"),
        _ => return pct(v),
    };
    format!("{name} {}", pct(v))
}

fn pct(v: f32) -> String {
    format!("{:.0} %", v)
}

/// Relance Dusk une seconde après sa fermeture (app installée seulement, pas
/// `target/`). Renvoie `false` si la relance n'a pas pu être programmée.
fn relaunch() -> bool {
    let bundle = objc2_foundation::NSBundle::mainBundle().bundlePath().to_string();
    if !bundle.ends_with(".app") {
        return false;
    }
    std::process::Command::new("/bin/sh")
        .args(["-c", "sleep 1; /usr/bin/open \"$0\"", &bundle])
        .spawn()
        .is_ok()
}

fn register_hotkeys() -> Option<Hotkeys> {
    let manager = GlobalHotKeyManager::new().map_err(|e| eprintln!("[dusk] raccourcis : {e}")).ok()?;
    let mods = Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::META);
    let up = HotKey::new(mods, Code::ArrowUp);
    let down = HotKey::new(mods, Code::ArrowDown);
    for hk in [up, down] {
        if let Err(e) = manager.register(hk) {
            eprintln!("[dusk] raccourci indisponible : {e}");
        }
    }
    Some(Hotkeys { _manager: manager, up: up.id(), down: down.id() })
}

pub fn delegate_object(d: &AppDelegate) -> &ProtocolObject<dyn NSApplicationDelegate> {
    ProtocolObject::from_ref(d)
}
