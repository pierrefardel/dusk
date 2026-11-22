//! Style F1 « Minuit magique » : couleurs, police et fabriques de vues AppKit.

use objc2::rc::Retained;
use objc2::{AllocAnyThread, ClassType, MainThreadMarker, MainThreadOnly, Message};
use objc2_foundation::NSObjectProtocol;
use objc2_app_kit::{
    NSBox, NSBoxType, NSColor, NSFont, NSFontWeightRegular, NSFontWeightSemibold, NSImage,
    NSLayoutConstraintOrientation, NSStackView, NSStackViewGravity, NSTextField, NSTitlePosition,
    NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSData, NSEdgeInsets, NSSize, NSString};

// Grille de 8 px : tous les espacements, marges et tailles en sont des multiples
// (4 px toléré uniquement pour les écarts très serrés).
pub const HALF: f64 = 4.0;
pub const S1: f64 = 8.0;
pub const S2: f64 = 16.0;
pub const S3: f64 = 24.0;
pub const S4: f64 = 32.0;
pub const S6: f64 = 48.0;

pub fn rgba(hex: u32, a: f64) -> Retained<NSColor> {
    let c = |s: u32| ((hex >> s) & 0xFF) as f64 / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(c(16), c(8), c(0), a)
}

// --- Thèmes ------------------------------------------------------------
//
// Trois thèmes, chacun en variante sombre et claire. Les vues ne connaissent
// que les rôles (fond, carte, action…), jamais les teintes.

#[derive(Clone, Copy)]
struct Palette {
    window: u32,
    surface: u32,
    card: u32,
    track: u32,
    accent: u32,
    on_accent: u32,
    text: u32,
    secondary: u32,
    muted: u32,
    faint: u32,
    spark: u32,
    ok: u32,
    warn: u32,
    displays: [u32; 4],
    gains: [u32; 3],
}

const DISPLAYS_DARK: [u32; 4] = [0x7CC4FF, 0xFF9EC7, 0x7FD69B, 0xFFB86B];
const DISPLAYS_LIGHT: [u32; 4] = [0x1F7FD6, 0xD6457F, 0x2E9E57, 0xC2741A];
const GAINS_DARK: [u32; 3] = [0xFF8A8A, 0x7FD69B, 0x7CC4FF];
const GAINS_LIGHT: [u32; 3] = [0xD6453D, 0x2E9E57, 0x1F6FD1];

const fn dark(window: u32, surface: u32, card: u32, track: u32, accent: u32, on_accent: u32, text: u32, secondary: u32, muted: u32, faint: u32) -> Palette {
    Palette { window, surface, card, track, accent, on_accent, text, secondary, muted, faint, spark: 0xFFD36E, ok: 0x7FD69B, warn: 0xF5C46A, displays: DISPLAYS_DARK, gains: GAINS_DARK }
}

const fn light(window: u32, surface: u32, card: u32, track: u32, accent: u32, on_accent: u32, text: u32, secondary: u32, muted: u32, faint: u32) -> Palette {
    Palette { window, surface, card, track, accent, on_accent, text, secondary, muted, faint, spark: 0xB8860B, ok: 0x1E8A4C, warn: 0xA86A00, displays: DISPLAYS_LIGHT, gains: GAINS_LIGHT }
}

/// Identifiants des thèmes, dans l'ordre d'affichage.
pub const THEMES: [&str; 3] = ["minuit", "ambre", "brume"];

fn palette_of(theme: &str, is_dark: bool) -> Palette {
    match (theme, is_dark) {
        ("ambre", true) => dark(0x1B1C21, 0x24262D, 0x2C2F38, 0x3F4250, 0xF5B544, 0x1B1C21, 0xF2F2F4, 0xC4C4CC, 0xFFD27A, 0x8E8E98),
        ("ambre", false) => light(0xFBF8F2, 0xF2ECDF, 0xFFFFFF, 0xE8E0CE, 0x9A5B00, 0xFFFFFF, 0x24211C, 0x6B6457, 0x8A5200, 0x9A9284),
        ("brume", true) => dark(0x16191E, 0x1E2228, 0x262B33, 0x343B46, 0x5AB0FF, 0x0D1620, 0xEEF2F7, 0xA9B3C1, 0x9CCBFF, 0x7C8594),
        ("brume", false) => light(0xF4F6F9, 0xE9EDF2, 0xFFFFFF, 0xDCE2EA, 0x1F6FD1, 0xFFFFFF, 0x1A1F26, 0x5A6573, 0x1A5DB0, 0x8C96A3),
        (_, false) => light(0xF5F3FF, 0xEAE6FF, 0xFFFFFF, 0xDDD7FF, 0x5B4BDB, 0xFFFFFF, 0x1E1A3D, 0x5E5880, 0x4A3FB0, 0x8A85A8),
        _ => dark(0x1A1733, 0x221E44, 0x2C2758, 0x3D3772, 0x8B7CFF, 0x15132B, 0xF3F1FF, 0xB9B4DC, 0xD6D0FF, 0x8F89B8),
    }
}

/// Thème et apparence choisis (« dark », « light », « system »).
static CHOICE: std::sync::Mutex<(String, String)> = std::sync::Mutex::new((String::new(), String::new()));

pub fn set(theme: &str, appearance: &str) {
    *CHOICE.lock().unwrap() = (theme.to_string(), appearance.to_string());
}

/// Variante sombre ? « Système » suit le réglage de macOS.
pub fn is_dark() -> bool {
    let appearance = CHOICE.lock().unwrap().1.clone();
    match appearance.as_str() {
        "light" => false,
        "system" => system_is_dark(),
        _ => true,
    }
}

fn system_is_dark() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return true };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let names = objc2_foundation::NSArray::from_slice(&[unsafe { objc2_app_kit::NSAppearanceNameAqua }, unsafe {
        objc2_app_kit::NSAppearanceNameDarkAqua
    }]);
    app.effectiveAppearance()
        .bestMatchFromAppearancesWithNames(&names)
        .is_some_and(|n| &*n == unsafe { objc2_app_kit::NSAppearanceNameDarkAqua })
}

fn palette() -> Palette {
    let theme = CHOICE.lock().unwrap().0.clone();
    palette_of(&theme, is_dark())
}

/// Apparence AppKit assortie (contrôles système : interrupteurs, menus…).
pub fn appearance() -> Option<Retained<objc2_app_kit::NSAppearance>> {
    let name = unsafe { if is_dark() { objc2_app_kit::NSAppearanceNameDarkAqua } else { objc2_app_kit::NSAppearanceNameAqua } };
    objc2_app_kit::NSAppearance::appearanceNamed(name)
}

/// Couleurs d'aperçu d'un thème : (fond, carte, piste, action) en sombre puis en clair.
pub fn preview(theme: &str) -> [[u32; 4]; 2] {
    [true, false].map(|d| {
        let p = palette_of(theme, d);
        [p.window, p.card, p.track, p.accent]
    })
}

/// Couleur d'action (curseur commun, sélection).
pub fn accent() -> Retained<NSColor> {
    rgba(palette().accent, 1.0)
}
/// Texte posé sur la couleur d'action.
pub fn on_accent() -> Retained<NSColor> {
    rgba(palette().on_accent, 1.0)
}
/// Étincelle : réservée à la baguette et aux repères mis en avant.
pub fn spark() -> Retained<NSColor> {
    rgba(palette().spark, 1.0)
}
/// Fond du popover, du HUD et de la barre latérale.
pub fn surface() -> Retained<NSColor> {
    rgba(palette().surface, 1.0)
}
/// Fond des cartes.
pub fn card_bg() -> Retained<NSColor> {
    rgba(palette().card, 1.0)
}
/// Fond de la fenêtre Réglages.
pub fn window_bg() -> Retained<NSColor> {
    rgba(palette().window, 1.0)
}
pub fn track() -> Retained<NSColor> {
    rgba(palette().track, 1.0)
}
pub fn text() -> Retained<NSColor> {
    rgba(palette().text, 1.0)
}
pub fn secondary() -> Retained<NSColor> {
    rgba(palette().secondary, 1.0)
}
pub fn muted() -> Retained<NSColor> {
    rgba(palette().muted, 1.0)
}
pub fn faint() -> Retained<NSColor> {
    rgba(palette().faint, 1.0)
}
/// État positif / à corriger (ex. autorisation des touches).
pub fn ok() -> Retained<NSColor> {
    rgba(palette().ok, 1.0)
}
pub fn warn() -> Retained<NSColor> {
    rgba(palette().warn, 1.0)
}
/// Curseurs rouge, vert, bleu.
pub fn gain(i: usize) -> Retained<NSColor> {
    rgba(palette().gains[i % 3], 1.0)
}

/// Une couleur par écran, dans l'ordre de détection.
pub fn display_color(i: usize) -> Retained<NSColor> {
    let d = palette().displays;
    rgba(d[i % d.len()], 1.0)
}

/// Police système arrondie (SF Rounded).
pub fn rounded(base: Retained<NSFont>, size: f64) -> Retained<NSFont> {
    let design = unsafe { objc2_app_kit::NSFontDescriptorSystemDesignRounded };
    base.fontDescriptor()
        .fontDescriptorWithDesign(design)
        .and_then(|d| NSFont::fontWithDescriptor_size(&d, size))
        .unwrap_or(base)
}

pub fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

pub fn label(mtm: MainThreadMarker, s: &str, size: f64, bold: bool, color: &NSColor) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&ns(s), mtm);
    let weight = unsafe { if bold { NSFontWeightSemibold } else { NSFontWeightRegular } };
    l.setFont(Some(&rounded(NSFont::systemFontOfSize_weight(size, weight), size)));
    l.setTextColor(Some(color));
    l
}

/// Texte sur plusieurs lignes, limité à `width`.
pub fn wrapping(mtm: MainThreadMarker, s: &str, size: f64, color: &NSColor, width: f64) -> Retained<NSTextField> {
    let l = NSTextField::wrappingLabelWithString(&ns(s), mtm);
    l.setFont(Some(&rounded(NSFont::systemFontOfSize_weight(size, unsafe { NSFontWeightRegular }), size)));
    l.setTextColor(Some(color));
    l.setPreferredMaxLayoutWidth(width);
    fixed_width(&l, width);
    l
}

/// Libellé à chiffres de largeur fixe (évite que « 9 % » / « 10 % » fasse sauter la mise en page).
pub fn value_label(mtm: MainThreadMarker, s: &str, size: f64) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&ns(s), mtm);
    l.setFont(Some(&rounded(NSFont::monospacedDigitSystemFontOfSize_weight(size, unsafe { NSFontWeightSemibold }), size)));
    l.setTextColor(Some(&muted()));
    l
}

pub fn stack(mtm: MainThreadMarker, vertical: bool, spacing: f64) -> Retained<NSStackView> {
    let s = NSStackView::new(mtm);
    s.setOrientation(if vertical {
        NSUserInterfaceLayoutOrientation::Vertical
    } else {
        NSUserInterfaceLayoutOrientation::Horizontal
    });
    s.setSpacing(spacing);
    if vertical {
        s.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    } else {
        s.setAlignment(objc2_app_kit::NSLayoutAttribute::CenterY);
    }
    s
}

pub fn insets(s: &NSStackView, v: f64, h: f64) {
    s.setEdgeInsets(NSEdgeInsets { top: v, left: h, bottom: v, right: h });
}

/// Rangée horizontale « gauche … droite » occupant toute la largeur.
pub fn spread(mtm: MainThreadMarker, left: &[&NSView], right: &[&NSView]) -> Retained<NSStackView> {
    let row = stack(mtm, false, S1);
    for v in left {
        row.addView_inGravity(v, NSStackViewGravity::Leading);
    }
    for v in right {
        row.addView_inGravity(v, NSStackViewGravity::Trailing);
    }
    row
}

pub fn fixed_width(v: &NSView, w: f64) {
    v.widthAnchor().constraintEqualToConstant(w).setActive(true);
    v.setContentHuggingPriority_forOrientation(1000.0, NSLayoutConstraintOrientation::Horizontal);
}

/// Carte à fond légèrement éclairci, coins arrondis.
///
/// Le contenu est épinglé aux bords par contraintes : sans elles, NSBox ne
/// transmet pas la taille de son contenu à Auto Layout (carte étirée ou écrasée).
pub fn card(mtm: MainThreadMarker, content: &NSView, fill: &NSColor, radius: f64, margin: f64) -> Retained<NSBox> {
    card_xy(mtm, content, fill, radius, margin, margin)
}

/// Carte à marges horizontale `mx` et verticale `my` distinctes.
pub fn card_xy(mtm: MainThreadMarker, content: &NSView, fill: &NSColor, radius: f64, mx: f64, my: f64) -> Retained<NSBox> {
    // Une carte qui entoure un bouton le déclenche sur toute sa surface (pas seulement le texte).
    let b: Retained<NSBox> = Retained::into_super(ClickBox::new(mtm));
    b.setBoxType(NSBoxType::Custom);
    b.setTitlePosition(NSTitlePosition::NoTitle);
    b.setBorderWidth(0.0);
    b.setCornerRadius(radius);
    b.setFillColor(fill);
    b.setContentViewMargins(NSSize::new(0.0, 0.0));
    b.setContentView(Some(content));
    pin_xy(&b, content, mx, my);
    // La carte épouse son contenu au lieu d'absorber l'espace libre d'une pile.
    b.setContentHuggingPriority_forOrientation(750.0, NSLayoutConstraintOrientation::Vertical);
    b
}

/// Épingle `child` (déjà sous-vue de `parent`) à ses quatre bords, avec une marge.
pub fn pin(parent: &NSView, child: &NSView, margin: f64) {
    pin_xy(parent, child, margin, margin);
}

pub fn pin_xy(parent: &NSView, child: &NSView, mx: f64, my: f64) {
    child.setTranslatesAutoresizingMaskIntoConstraints(false);
    child.leadingAnchor().constraintEqualToAnchor_constant(&parent.leadingAnchor(), mx).setActive(true);
    child.trailingAnchor().constraintEqualToAnchor_constant(&parent.trailingAnchor(), -mx).setActive(true);
    child.topAnchor().constraintEqualToAnchor_constant(&parent.topAnchor(), my).setActive(true);
    child.bottomAnchor().constraintEqualToAnchor_constant(&parent.bottomAnchor(), -my).setActive(true);
}

/// Puce sélectionnable (mode d'image, couleur, entrée) : violette quand choisie.
pub fn chip(mtm: MainThreadMarker, button: &objc2_app_kit::NSButton, selected: bool) -> Retained<NSBox> {
    button.setBordered(false);
    let weight = unsafe { if selected { objc2_app_kit::NSFontWeightBold } else { NSFontWeightSemibold } };
    button.setFont(Some(&rounded(NSFont::systemFontOfSize_weight(13.0, weight), 13.0)));
    button.setContentTintColor(Some(&*if selected { on_accent() } else { text() }));
    card_xy(mtm, button, &*if selected { accent() } else { track() }, S2, S2, S1)
}

/// Range des vues en lignes successives qui ne dépassent pas `width` (puces).
pub fn wrap(mtm: MainThreadMarker, views: &[Retained<NSView>], width: f64) -> Retained<NSStackView> {
    let rows = stack(mtm, true, S1);
    let mut row = stack(mtm, false, S1);
    let mut used = 0.0;
    for v in views {
        let w = v.fittingSize().width;
        if used > 0.0 && used + S1 + w > width {
            rows.addArrangedSubview(&row);
            row = stack(mtm, false, S1);
            used = 0.0;
        }
        used += if used > 0.0 { S1 + w } else { w };
        row.addArrangedSubview(v);
    }
    rows.addArrangedSubview(&row);
    rows
}

objc2::define_class!(
    /// Vue à origine en haut : le contenu d'un défilement démarre en haut.
    #[unsafe(super(NSView))]
    #[thread_kind = objc2::MainThreadOnly]
    #[name = "DuskFlippedView"]
    pub struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

objc2::define_class!(
    /// Carte dont toute la surface déclenche le bouton qu'elle contient : les marges
    /// autour du texte font partie de la zone de clic.
    #[unsafe(super(NSBox, NSView))]
    #[thread_kind = objc2::MainThreadOnly]
    #[name = "DuskClickBox"]
    pub struct ClickBox;

    impl ClickBox {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &objc2_app_kit::NSEvent) {
            let button = self.contentView().and_then(|v| sole_button(&v)).filter(|b| b.isEnabled());
            match button {
                Some(b) => unsafe { b.performClick(None) },
                None => unsafe { objc2::msg_send![super(self), mouseDown: event] },
            }
        }
    }
);

/// Le bouton d'une carte : la vue elle-même, ou l'unique bouton d'une rangée qui ne
/// contient sinon que du texte ou une image (onglet avec pastille de couleur).
fn sole_button(v: &NSView) -> Option<Retained<objc2_app_kit::NSButton>> {
    if let Ok(b) = v.retain().downcast::<objc2_app_kit::NSButton>() {
        return Some(b);
    }
    let mut found = None;
    for sub in v.subviews().iter() {
        if let Ok(b) = sub.clone().downcast::<objc2_app_kit::NSButton>() {
            if found.replace(b).is_some() {
                return None;
            }
        } else if !(sub.isKindOfClass(NSTextField::class()) || sub.isKindOfClass(objc2_app_kit::NSImageView::class())) {
            return None;
        }
    }
    found
}

impl ClickBox {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { objc2::msg_send![Self::alloc(mtm), init] }
    }
}

/// Curseur de luminosité : descend sous 0 (« Plus sombre »), un petit trait marque
/// le minimum de l'écran.
pub fn dim_range(s: &objc2_app_kit::NSSlider) {
    let mtm = MainThreadMarker::from(s);
    s.setMinValue(dusk_core::DIM_MIN as f64);
    let mark = NSBox::new(mtm);
    mark.setBoxType(NSBoxType::Custom);
    mark.setTitlePosition(NSTitlePosition::NoTitle);
    mark.setBorderWidth(0.0);
    mark.setCornerRadius(1.0);
    mark.setFillColor(&muted());
    mark.setTranslatesAutoresizingMaskIntoConstraints(false);
    s.addSubview(&mark);
    mark.widthAnchor().constraintEqualToConstant(2.0).setActive(true);
    mark.heightAnchor().constraintEqualToConstant(S1 + HALF).setActive(true);
    mark.centerYAnchor().constraintEqualToAnchor(&s.centerYAnchor()).setActive(true);
    // Position de 0 sur la piste, bouton compris (demi-bouton ≈ 10 px de chaque côté).
    let f = -dusk_core::DIM_MIN as f64 / (100.0 - dusk_core::DIM_MIN as f64);
    let knob = 10.0;
    unsafe {
        objc2_app_kit::NSLayoutConstraint::constraintWithItem_attribute_relatedBy_toItem_attribute_multiplier_constant(
            &mark,
            objc2_app_kit::NSLayoutAttribute::CenterX,
            objc2_app_kit::NSLayoutRelation::Equal,
            Some(s),
            objc2_app_kit::NSLayoutAttribute::Trailing,
            f,
            knob * (1.0 - 2.0 * f),
        )
    }
    .setActive(true);
}

fn pill(mtm: MainThreadMarker, fill: &NSColor, w: f64, h: f64) -> Retained<NSBox> {
    let b = NSBox::new(mtm);
    b.setBoxType(NSBoxType::Custom);
    b.setTitlePosition(NSTitlePosition::NoTitle);
    b.setBorderWidth(0.0);
    b.setCornerRadius(h / 2.0);
    b.setFillColor(fill);
    b.setTranslatesAutoresizingMaskIntoConstraints(false);
    b.widthAnchor().constraintEqualToConstant(w).setActive(true);
    b.heightAnchor().constraintEqualToConstant(h).setActive(true);
    b
}

/// Jauge en pilule (HUD) : piste arrondie et remplissage arrondi, `fraction` de 0 à 1.
pub fn pill_bar(mtm: MainThreadMarker, fraction: f64, width: f64, fill: &NSColor) -> Retained<NSBox> {
    let h = S1;
    let track_box = pill(mtm, &track(), width, h);
    let filled = pill(mtm, fill, (width * fraction.clamp(0.0, 1.0)).max(h), h);
    track_box.addSubview(&filled);
    filled.leadingAnchor().constraintEqualToAnchor(&track_box.leadingAnchor()).setActive(true);
    filled.centerYAnchor().constraintEqualToAnchor(&track_box.centerYAnchor()).setActive(true);
    track_box
}

/// Touche de clavier dessinée (pilule bordée, police à chasse fixe).
pub fn kbd(mtm: MainThreadMarker, key: &str) -> Retained<NSBox> {
    let l = NSTextField::labelWithString(&ns(key), mtm);
    // Les symboles (⌥ ⇧ ⌃ ⌘ ↑ ↓) sont petits dans une police à chasse fixe : police système.
    let size = if key.chars().count() == 1 { 13.0 } else { 12.0 };
    l.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(size, unsafe { NSFontWeightSemibold })));
    if key.chars().all(|c| !c.is_ascii()) {
        l.setFont(Some(&NSFont::systemFontOfSize_weight(size, unsafe { NSFontWeightSemibold })));
    }
    l.setTextColor(Some(&text()));
    l.setAlignment(objc2_app_kit::NSTextAlignment::Center);
    let b = card_xy(mtm, &l, &track(), S3 / 2.0, S1 + HALF, HALF);
    b.setBorderWidth(1.0);
    b.setBorderColor(&faint());
    fixed_height(&b, S3);
    b.widthAnchor().constraintGreaterThanOrEqualToConstant(S4 + S1).setActive(true);
    b.setContentHuggingPriority_forOrientation(750.0, NSLayoutConstraintOrientation::Horizontal);
    b
}

/// Combinaison de touches : groupes reliés par « + », variantes d'un groupe par « / ».
/// Ex. `[["⌥"], ["F1", "F2"]]` → ⌥ + F1 / F2.
pub fn kbd_combo(mtm: MainThreadMarker, groups: &[&[&str]]) -> Retained<NSStackView> {
    let row = stack(mtm, false, S1);
    let sep = |s: &str| label(mtm, s, 12.0, false, &secondary());
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            row.addArrangedSubview(&sep("+"));
        }
        for (j, key) in group.iter().enumerate() {
            if j > 0 {
                row.addArrangedSubview(&sep("/"));
            }
            row.addArrangedSubview(&kbd(mtm, key));
        }
    }
    row
}

/// Zone de clic d'au moins 32 px de haut pour un bouton sans carte (lien, icône).
pub fn roomy(b: &objc2_app_kit::NSButton) {
    b.heightAnchor().constraintGreaterThanOrEqualToConstant(S4).setActive(true);
}

impl FlippedView {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { objc2::msg_send![Self::alloc(mtm), init] }
    }
}

/// Hauteur fixe (éléments de liste qui ne doivent pas s'étirer).
pub fn fixed_height(v: &NSView, h: f64) {
    v.heightAnchor().constraintEqualToConstant(h).setActive(true);
}

/// Laisse une pile être plus haute que son contenu (l'espace reste en bas).
pub fn loose_vertical(s: &NSStackView) {
    s.setHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Vertical);
}

pub fn separator(mtm: MainThreadMarker) -> Retained<NSBox> {
    let b = NSBox::new(mtm);
    b.setBoxType(NSBoxType::Separator);
    b
}

/// Icônes Hugeicons (jeu gratuit, MIT, `assets/icons/`), en image modèle :
/// teintées par `setContentTintColor`, donc toujours aux couleurs du thème.
#[derive(Clone, Copy)]
pub enum Icon {
    SunDim,
    Sun,
    ChevronDown,
    ChevronRight,
    Undo,
    Settings,
    Power,
    Tv,
    Bulb,
}

pub fn icon(which: Icon, size: f64) -> Retained<NSImage> {
    let svg: &[u8] = match which {
        Icon::SunDim => include_bytes!("../../../assets/icons/sun-dim.svg"),
        Icon::Sun => include_bytes!("../../../assets/icons/sun.svg"),
        Icon::ChevronDown => include_bytes!("../../../assets/icons/chevron-down.svg"),
        Icon::ChevronRight => include_bytes!("../../../assets/icons/chevron-right.svg"),
        Icon::Undo => include_bytes!("../../../assets/icons/undo.svg"),
        Icon::Settings => include_bytes!("../../../assets/icons/settings.svg"),
        Icon::Power => include_bytes!("../../../assets/icons/power.svg"),
        Icon::Tv => include_bytes!("../../../assets/icons/tv.svg"),
        Icon::Bulb => include_bytes!("../../../assets/icons/bulb.svg"),
    };
    let img = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(svg)).expect("icône SVG");
    img.setSize(NSSize::new(size, size));
    img.setTemplate(true);
    img
}

const MENUBAR_SVG: &[u8] = include_bytes!("../../../assets/menubar.svg");

/// Glyphe « source et halos » (d'après AppIcon.svg), image modèle (s'adapte au thème de la barre des menus).
pub fn wand_image() -> Retained<NSImage> {
    let data = NSData::with_bytes(MENUBAR_SVG);
    let img = NSImage::initWithData(NSImage::alloc(), &data)
        .expect("icône de barre des menus");
    img.setSize(NSSize::new(18.0, 18.0));
    img.setTemplate(true);
    img
}

/// Enregistre le rendu d'une vue en PNG (aperçus de développement, sans autorisation d'écran).
/// Rendu en 2x (Retina) quel que soit l'écran ; `DUSK_SNAPSHOT_SCALE` le change.
pub fn snapshot(view: &NSView, path: &str) {
    use objc2_app_kit::{NSBitmapImageRep, NSCalibratedRGBColorSpace};
    let rect = view.bounds();
    let scale: f64 = std::env::var("DUSK_SNAPSHOT_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            (rect.size.width * scale).round() as isize,
            (rect.size.height * scale).round() as isize,
            8,
            4,
            true,
            false,
            NSCalibratedRGBColorSpace,
            0,
            0,
        )
    };
    let Some(rep) = rep else { return };
    // Taille en points : le rendu remplit alors les pixels à l'échelle voulue.
    rep.setSize(rect.size);
    view.cacheDisplayInRect_toBitmapImageRep(rect, &rep);
    let props = objc2_foundation::NSDictionary::new();
    if let Some(data) = unsafe { rep.representationUsingType_properties(objc2_app_kit::NSBitmapImageFileType::PNG, &props) } {
        data.writeToFile_atomically(&ns(path), true);
    }
}
