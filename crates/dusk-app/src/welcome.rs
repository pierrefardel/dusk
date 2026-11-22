//! Accueil du premier lancement : écrans trouvés, autorisation des touches,
//! ouverture au démarrage. Une seule fenêtre, deux étapes, puis la bulle « C'est prêt ».

use crate::i18n::t;
use crate::mediakeys;
use crate::theme::{self, ns, Pill};
use dusk_core::{Engine, Welcome as Stage};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearanceCustomization, NSApplication, NSBackingStoreType, NSButton, NSControlStateValueOn, NSImageView,
    NSStackView, NSSwitch, NSTextField, NSView, NSWindow, NSWindowButton, NSWindowDelegate, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use std::cell::{Cell, OnceCell, RefCell};
use std::sync::Arc;
use std::time::Instant;

const WIDTH: f64 = 448.0;
/// Largeur utile (marges de 32 px).
const INNER: f64 = WIDTH - 2.0 * theme::S4;
/// Largeur utile dans une carte (marges de 16 px).
const CARD_INNER: f64 = INNER - 2.0 * theme::S2;
const ICON: f64 = 56.0;

pub struct Ivars {
    engine: Arc<Engine>,
    /// Appelé à la fin de l'accueil ; `true` : les touches sont autorisées.
    on_finish: Box<dyn Fn(bool)>,
    window: OnceCell<Retained<NSWindow>>,
    /// 0 : bienvenue ; 1 : en attente de macOS.
    step: Cell<usize>,
    login: RefCell<Option<Retained<NSSwitch>>>,
    /// Point qui pulse pendant l'attente.
    pulse: RefCell<Option<Retained<NSTextField>>>,
    started: Instant,
    finished: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DuskWelcome"]
    #[ivars = Ivars]
    pub struct Welcome;

    unsafe impl NSObjectProtocol for Welcome {}

    unsafe impl NSWindowDelegate for Welcome {
        /// Fermer la fenêtre revient à « Plus tard ».
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            if !self.ivars().finished.get() {
                self.apply_login();
                self.finish(false, false);
            }
        }
    }

    impl Welcome {
        #[unsafe(method(allow:))]
        fn allow(&self, _sender: Option<&AnyObject>) {
            self.apply_login();
            self.ivars().engine.update_config(|c| c.welcome = Stage::Waiting);
            mediakeys::open_accessibility();
            self.ivars().step.set(1);
            self.render();
        }

        /// Touches déjà autorisées (réinstallation) : directement la bulle.
        #[unsafe(method(start:))]
        fn start(&self, _sender: Option<&AnyObject>) {
            self.apply_login();
            self.finish(true, true);
        }

        #[unsafe(method(later:))]
        fn later(&self, _sender: Option<&AnyObject>) {
            if self.ivars().step.get() == 0 {
                self.apply_login();
            }
            self.finish(false, true);
        }

        #[unsafe(method(reopen:))]
        fn reopen(&self, _sender: Option<&AnyObject>) {
            mediakeys::open_accessibility();
        }
    }
);

impl Welcome {
    pub fn new(mtm: MainThreadMarker, engine: Arc<Engine>, on_finish: Box<dyn Fn(bool)>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            engine,
            on_finish,
            window: OnceCell::new(),
            step: Cell::new(0),
            login: RefCell::new(None),
            pulse: RefCell::new(None),
            started: Instant::now(),
            finished: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn target(&self) -> &AnyObject {
        self
    }

    /// Affiche l'étape `step` (0 : bienvenue, 1 : attente ; aperçus).
    pub fn show(&self, step: usize) {
        let mtm = self.mtm();
        self.ivars().step.set(step.min(1));
        let window = self.ivars().window.get_or_init(|| self.build_window());
        self.render();
        window.center();
        window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }

    pub fn is_open(&self) -> bool {
        !self.ivars().finished.get() && self.ivars().window.get().is_some_and(|w| w.isVisible())
    }

    /// L'autorisation est arrivée sans relance (app lancée hors bundle).
    pub fn granted(&self) {
        self.finish(true, true);
    }

    pub fn content_view(&self) -> Option<Retained<NSView>> {
        self.ivars().window.get().and_then(|w| w.contentView())
    }

    /// Fait pulser le point d'attente (appelé toutes les 50 ms).
    pub fn tick(&self) {
        if let Some(dot) = self.ivars().pulse.borrow().as_ref() {
            let phase = self.ivars().started.elapsed().as_secs_f64() * std::f64::consts::TAU / 1.6;
            dot.setAlphaValue(0.35 + 0.65 * (0.5 - 0.5 * phase.cos()));
        }
    }

    fn finish(&self, keys: bool, close: bool) {
        if self.ivars().finished.replace(true) {
            return;
        }
        self.ivars().engine.update_config(|c| c.welcome = Stage::Done);
        *self.ivars().pulse.borrow_mut() = None;
        if close {
            if let Some(w) = self.ivars().window.get() {
                w.close();
            }
        }
        (self.ivars().on_finish)(keys);
    }

    /// Ouverture au démarrage, cochée d'avance (app installée seulement).
    fn apply_login(&self) {
        let on = self.ivars().login.borrow().as_ref().is_some_and(|s| s.state() == NSControlStateValueOn);
        let bundle = objc2_foundation::NSBundle::mainBundle().bundlePath().to_string();
        if !bundle.ends_with(".app") {
            return;
        }
        let service = unsafe { SMAppService::mainAppService() };
        let enabled = unsafe { service.status() } == SMAppServiceStatus::Enabled;
        let r = match (on, enabled) {
            (true, false) => unsafe { service.registerAndReturnError() },
            (false, true) => unsafe { service.unregisterAndReturnError() },
            _ => Ok(()),
        };
        if let Err(e) = r {
            eprintln!("[dusk] ouverture de session : {e:?}");
        }
    }

    fn build_window(&self) -> Retained<NSWindow> {
        let mtm = self.mtm();
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::FullSizeContentView;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, 480.0)),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&ns(t("welcome.title")));
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
        for b in [NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton] {
            if let Some(b) = window.standardWindowButton(b) {
                b.setHidden(true);
            }
        }
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        window
    }

    fn render(&self) {
        let Some(window) = self.ivars().window.get() else { return };
        window.setAppearance(theme::appearance().as_deref());
        window.setBackgroundColor(Some(&theme::surface()));
        *self.ivars().pulse.borrow_mut() = None;
        let root = if self.ivars().step.get() == 0 { self.page_welcome() } else { self.page_wait() };
        theme::fixed_width(&root, WIDTH);
        root.setEdgeInsets(objc2_foundation::NSEdgeInsets {
            top: theme::S4 + theme::S1,
            left: theme::S4,
            bottom: theme::S4,
            right: theme::S4,
        });
        // Le haut de la fenêtre reste en place quand sa hauteur change.
        let old = window.frame();
        let bg = theme::card(self.mtm(), &root, &theme::surface(), 0.0, 0.0);
        window.setContentView(Some(&bg));
        bg.layoutSubtreeIfNeeded();
        let size = bg.fittingSize();
        let frame = window.frameRectForContentRect(NSRect::new(NSPoint::new(0.0, 0.0), size));
        let top = old.origin.y + old.size.height;
        window.setFrame_display(
            NSRect::new(NSPoint::new(old.origin.x, top - frame.size.height), frame.size),
            true,
        );
    }

    fn page_welcome(&self) -> Retained<NSStackView> {
        let mtm = self.mtm();
        let root = theme::stack(mtm, true, theme::S3);
        root.addArrangedSubview(&self.header(t("welcome.title"), t("welcome.lead")));

        let cards = theme::stack(mtm, true, theme::S2);
        let displays = self.ivars().engine.displays();
        if !displays.is_empty() {
            let chips: Vec<Retained<NSView>> = displays
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let row = theme::stack(mtm, false, theme::S1);
                    row.addArrangedSubview(&theme::label(mtm, "●", 10.0, false, &theme::display_color(i)));
                    row.addArrangedSubview(&theme::label(mtm, &d.name, 13.0, true, &theme::text()));
                    let chip = theme::card_xy(mtm, &row, &theme::track(), theme::S2, theme::S2, theme::S1);
                    Retained::into_super(chip)
                })
                .collect();
            let body = theme::stack(mtm, true, theme::S1);
            body.addArrangedSubview(&theme::label(mtm, t("welcome.displays"), 13.0, true, &theme::text()));
            body.addArrangedSubview(&theme::wrap(mtm, &chips, CARD_INNER));
            theme::fixed_width(&body, CARD_INNER);
            cards.addArrangedSubview(&theme::card(mtm, &body, &theme::card_bg(), theme::S1 + theme::HALF, theme::S2));
        }

        let trusted = mediakeys::is_trusted(false);
        if !trusted {
            let icon = NSImageView::imageViewWithImage(&theme::icon(theme::Icon::Keyboard, 24.0), mtm);
            icon.setContentTintColor(Some(&theme::spark()));
            let width = CARD_INNER - 24.0 - theme::S2;
            let text = theme::stack(mtm, true, theme::HALF);
            text.addArrangedSubview(&theme::label(mtm, t("welcome.keys.title"), 13.0, true, &theme::text()));
            text.addArrangedSubview(&theme::wrapping(mtm, t("welcome.keys.text"), 12.0, &theme::secondary(), width));
            let row = theme::stack(mtm, false, theme::S2);
            row.setAlignment(objc2_app_kit::NSLayoutAttribute::Top);
            row.addArrangedSubview(&icon);
            row.addArrangedSubview(&text);
            theme::fixed_width(&row, CARD_INNER);
            cards.addArrangedSubview(&theme::card(mtm, &row, &theme::card_bg(), theme::S1 + theme::HALF, theme::S2));
        }

        let switch = NSSwitch::new(mtm);
        let previous = self.ivars().login.borrow().as_ref().map(|s| s.state());
        switch.setState(previous.unwrap_or(NSControlStateValueOn));
        let label = theme::label(mtm, t("welcome.login"), 13.0, true, &theme::text());
        let row = theme::spread(mtm, &[&label], &[&switch]);
        theme::fixed_width(&row, CARD_INNER);
        theme::fixed_height(&row, theme::S4 + theme::S1);
        let line = theme::card_xy(mtm, &row, &objc2_app_kit::NSColor::clearColor(), 0.0, theme::S2, 0.0);
        cards.addArrangedSubview(&line);
        *self.ivars().login.borrow_mut() = Some(switch);
        root.addArrangedSubview(&cards);

        let later = self.pill(t("welcome.later"), sel!(later:), Pill::Plain);
        let main = if trusted {
            self.pill(t("welcome.start"), sel!(start:), Pill::Primary)
        } else {
            self.pill(t("welcome.allow"), sel!(allow:), Pill::Primary)
        };
        root.addArrangedSubview(&self.buttons(&later, &main));
        root
    }

    fn page_wait(&self) -> Retained<NSStackView> {
        let mtm = self.mtm();
        let root = theme::stack(mtm, true, theme::S3);
        root.addArrangedSubview(&self.header(t("wait.title"), t("wait.lead")));

        // Ce qu'il faut activer, dessiné : la rubrique change de nom selon les versions de macOS.
        let icon = NSImageView::imageViewWithImage(&theme::app_icon(theme::S3), mtm);
        let name = theme::label(mtm, "Dusk", 13.0, true, &theme::text());
        let line = theme::spread(mtm, &[&icon, &name], &[&theme::drawn_switch(mtm, false)]);
        line.setSpacing(theme::S2);
        theme::fixed_width(&line, CARD_INNER - 2.0 * theme::S2);
        theme::fixed_height(&line, theme::S6);
        let row = theme::card_xy(mtm, &line, &theme::card_bg(), theme::S1 + theme::HALF, theme::S2, 0.0);
        row.setBorderWidth(2.0);
        row.setBorderColor(&theme::spark());
        let body = theme::stack(mtm, true, theme::S1);
        body.addArrangedSubview(&theme::label(mtm, t("wait.caption"), 11.0, true, &theme::faint()));
        body.addArrangedSubview(&row);
        theme::fixed_width(&body, CARD_INNER);
        root.addArrangedSubview(&theme::card(mtm, &body, &theme::window_bg(), theme::S1 + theme::HALF, theme::S2));

        let dot = theme::label(mtm, "●", 10.0, false, &theme::spark());
        let hint = theme::wrapping(mtm, t("wait.hint"), 12.0, &theme::secondary(), INNER - theme::S3);
        let wait = theme::stack(mtm, false, theme::S1);
        wait.addArrangedSubview(&dot);
        wait.addArrangedSubview(&hint);
        *self.ivars().pulse.borrow_mut() = Some(dot);
        root.addArrangedSubview(&wait);

        let later = self.pill(t("welcome.later"), sel!(later:), Pill::Plain);
        let reopen = self.pill(t("wait.reopen"), sel!(reopen:), Pill::Secondary);
        root.addArrangedSubview(&self.buttons(&later, &reopen));
        root
    }

    /// Icône de l'app, titre et phrase d'introduction.
    fn header(&self, title: &str, lead: &str) -> Retained<NSStackView> {
        let mtm = self.mtm();
        let icon = NSImageView::imageViewWithImage(&theme::app_icon(ICON), mtm);
        let text = theme::stack(mtm, true, theme::HALF);
        text.addArrangedSubview(&theme::label(mtm, title, 22.0, true, &theme::text()));
        text.addArrangedSubview(&theme::wrapping(mtm, lead, 13.0, &theme::secondary(), INNER - ICON - theme::S2));
        let row = theme::stack(mtm, false, theme::S2);
        row.addArrangedSubview(&icon);
        row.addArrangedSubview(&text);
        row
    }

    fn buttons(&self, left: &NSView, right: &NSView) -> Retained<NSStackView> {
        let row = theme::spread(self.mtm(), &[], &[left, right]);
        theme::fixed_width(&row, INNER);
        row
    }

    fn pill(&self, title: &str, action: Sel, kind: Pill) -> Retained<NSView> {
        let b = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), Some(self.target()), Some(action), self.mtm()) };
        Retained::into_super(theme::pill_button(self.mtm(), &b, kind))
    }
}
