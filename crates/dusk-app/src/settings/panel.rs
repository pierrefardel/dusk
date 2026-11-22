//! Page Écrans : panneau complet de l'écran choisi, d'après ce qu'il annonce.
//!
//! L'essentiel est visible (image, mode, couleur, son, entrée) ; le reste se
//! déplie avec « Plus de réglages ». Seuls les réglages acceptés sont affichés.

use super::{Settings, CONTENT};
use crate::i18n::{self, t, tx};
use crate::theme::{self, ns};
use dusk_core::panel::{self as caps, Choice, PRESET_USER_1, VCP_DISPLAY_MODE};
use dusk_core::{Control, DisplayState};
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSButton, NSPopUpButton, NSSlider, NSStackView, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};

/// Largeur d'une colonne, et largeur utile dans une carte de colonne.
const COL: f64 = (CONTENT - theme::S2) / 2.0;
const COL_INNER: f64 = COL - 2.0 * theme::S2;
const FULL_INNER: f64 = CONTENT - 2.0 * theme::S2;

/// Étiquettes des curseurs : écran × 256 + code VCP (0x10 pour la luminosité).
pub fn slider_tag(index: usize, code: u8) -> isize {
    (index * 256 + code as usize) as isize
}

/// Puces : écran, code VCP et valeur dans une seule étiquette.
pub fn choice_tag(index: usize, code: u8, value: u8) -> isize {
    ((index << 16) | ((code as usize) << 8) | value as usize) as isize
}

pub fn decode_choice(tag: isize) -> (usize, u8, u8) {
    let t = tag as usize;
    (t >> 16, (t >> 8) as u8, t as u8)
}

pub fn pct(v: f32) -> String {
    format!("{:.0} %", v)
}

impl Settings {
    pub(super) fn page_displays(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let displays = self.ivars().engine.displays();
        self.ivars().labels.borrow_mut().clear();

        let page = theme::stack(mtm, true, theme::S3);
        let head = theme::stack(mtm, true, theme::S1);
        head.addArrangedSubview(&theme::label(mtm, t("displays.title"), 20.0, true, &theme::text()));
        head.addArrangedSubview(&theme::label(mtm, t("displays.subtitle"), 13.0, false, &theme::secondary()));
        let index = self.ivars().display.get().min(displays.len().saturating_sub(1));
        let tabs = self.display_tabs(&displays, index);
        let top = theme::spread(mtm, &[&head], &[&tabs]);
        theme::fixed_width(&top, CONTENT);
        page.addArrangedSubview(&top);

        // Écrans éteints depuis Dusk : ils ne sont plus détectés, on propose de les rallumer.
        for s in self.ivars().engine.sleeping() {
            let b = self.bordered_button(t("wake.btn"), sel!(wakeDisplay:));
            b.setTag(s.id as isize);
            let row = self.action_row(&tx("sleeping.title", &s.name), t("sleeping.hint"), &b);
            theme::fixed_width(&row, FULL_INNER);
            page.addArrangedSubview(&self.card(mtm, &row, CONTENT));
        }

        let Some(d) = displays.get(index) else { return Retained::into_super(page) };
        let can_sleep = displays.len() > 1;

        let p = &d.panel;

        // Colonne gauche : image, mode d'image, son.
        let left = theme::stack(mtm, true, theme::S2);
        left.addArrangedSubview(&self.image_card(index, d));
        {
            if let Some(modes) = &p.picture_mode {
                let known = known_modes(modes, d.vendor);
                if !known.is_empty() {
                    left.addArrangedSubview(&self.mode_card(index, modes, &known));
                }
            }
        }
        if d.volume.is_some() || p.mute {
            left.addArrangedSubview(&self.sound_card(index, d, COL));
        }

        // Colonne droite : couleur, entrée.
        let right = theme::stack(mtm, true, theme::S2);
        if d.software {
            right.addArrangedSubview(&self.software_card());
        }
        {
            if p.color_preset.is_some() || p.rgb {
                right.addArrangedSubview(&self.color_card(index, d));
            }
            if let Some(input) = &p.input {
                right.addArrangedSubview(&self.input_card(index, input, d.vendor));
            }
        }
        let columns = theme::stack(mtm, false, theme::S2);
        columns.setAlignment(objc2_app_kit::NSLayoutAttribute::Top);
        // La colonne la plus courte reçoit un espaceur extensible : ses cartes ne s'étirent pas.
        for col in [&left, &right] {
            let spacer = NSView::new(mtm);
            spacer.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Vertical);
            col.addArrangedSubview(&spacer);
        }
        theme::fixed_width(&left, COL);
        columns.addArrangedSubview(&left);
        if right.arrangedSubviews().len() > 1 {
            theme::fixed_width(&right, COL);
            columns.addArrangedSubview(&right);
        }
        page.addArrangedSubview(&columns);

        let has_more = p.sharpness || p.osd_language.is_some() || can_sleep || p.factory_reset;
        if has_more {
            let open = self.ivars().more.get();
            let toggle = self.button(t(if open { "less" } else { "more" }), sel!(toggleMore:));
            theme::roomy(&toggle);
            toggle.setFont(Some(&theme::rounded(
                objc2_app_kit::NSFont::systemFontOfSize_weight(13.0, unsafe { objc2_app_kit::NSFontWeightBold }),
                13.0,
            )));
            toggle.setContentTintColor(Some(&theme::muted()));
            {
                let img = theme::icon(if open { theme::Icon::ChevronDown } else { theme::Icon::ChevronRight }, 14.0);
                toggle.setImage(Some(&img));
                toggle.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageLeading);
            }
            page.addArrangedSubview(&toggle);
            if open {
                page.addArrangedSubview(&self.more_card(index, d, can_sleep));
            }
        }

        // Écran qui ne répond pas : une note, et le choix d'afficher ses curseurs s'il n'annonce rien.
        if d.write_only {
            page.addArrangedSubview(&theme::wrapping(mtm, t("note.write_only"), 12.0, &theme::secondary(), CONTENT));
            if !p.known {
                let on = d.contrast.is_some() || d.volume.is_some();
                let check = self.checkbox(t("extras.check"), on, sel!(toggleExtras:));
                check.setTag(index as isize);
                page.addArrangedSubview(&check);
                page.addArrangedSubview(&theme::wrapping(mtm, t("extras.hint"), 11.0, &theme::secondary(), CONTENT));
            }
        }
        let footer = theme::label(mtm, t("footer"), 12.0, false, &theme::faint());
        // « Annuler mes changements » : actif dès qu'un réglage de cet écran a bougé.
        let undo = self.button(t("undo"), sel!(undoChanges:));
        theme::roomy(&undo);
        undo.setTag(index as isize);
        undo.setContentTintColor(Some(&theme::muted()));
        {
            let img = theme::icon(theme::Icon::Undo, 14.0);
            undo.setImage(Some(&img));
            undo.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageLeading);
        }
        undo.setEnabled(self.ivars().before.borrow().get(&d.key).is_some_and(|b| changed(b, d)));
        *self.ivars().undo.borrow_mut() = Some(undo.clone());
        let row = theme::spread(mtm, &[&footer], &[&undo]);
        theme::fixed_width(&row, CONTENT);
        page.addArrangedSubview(&row);
        Retained::into_super(page)
    }

    // --- Cartes ---------------------------------------------------------------

    fn display_tabs(&self, displays: &[DisplayState], current: usize) -> Retained<NSView> {
        let mtm = self.mtm();
        let row = theme::stack(mtm, false, theme::HALF);
        for (i, d) in displays.iter().enumerate() {
            let selected = i == current;
            let dot = theme::label(mtm, "●", 10.0, false, &theme::display_color(i));
            let b = self.button(&d.name, sel!(selectDisplay:));
            b.setTag(i as isize);
            b.setFont(Some(&theme::rounded(
                objc2_app_kit::NSFont::systemFontOfSize_weight(13.0, unsafe {
                    if selected { objc2_app_kit::NSFontWeightBold } else { objc2_app_kit::NSFontWeightSemibold }
                }),
                13.0,
            )));
            b.setContentTintColor(Some(&*if selected { theme::text() } else { theme::secondary() }));
            let inner = theme::stack(mtm, false, theme::S1);
            inner.addArrangedSubview(&dot);
            inner.addArrangedSubview(&b);
            let fill = if selected { theme::card_bg() } else { objc2_app_kit::NSColor::clearColor() };
            row.addArrangedSubview(&theme::card_xy(mtm, &inner, &fill, theme::S1 + theme::HALF, theme::S2, theme::S1));
        }
        Retained::into_super(theme::card(mtm, &row, &theme::surface(), theme::S2, theme::HALF))
    }

    fn image_card(&self, index: usize, d: &DisplayState) -> Retained<NSView> {
        let mtm = self.mtm();
        let body = self.card_body(t("card.image"));
        let color = theme::display_color(index);
        body.addArrangedSubview(&self.level(t("brightness"), d.brightness, slider_tag(index, 0x10), sel!(panelBrightness:), &color, COL_INNER, !d.software));
        if let Some(v) = d.contrast {
            body.addArrangedSubview(&self.level(t("contrast"), v, slider_tag(index, Control::Contrast.vcp()), sel!(panelControl:), &color, COL_INNER, false));
        }
        self.card(mtm, &body, COL)
    }

    fn mode_card(&self, index: usize, modes: &Choice, known: &[(u8, &'static str, Option<&'static str>)]) -> Retained<NSView> {
        let mtm = self.mtm();
        let body = self.card_body(t("card.mode"));
        let chips: Vec<Retained<NSView>> = known
            .iter()
            .map(|&(v, name, _)| self.chip(name, choice_tag(index, modes.code, v), modes.current == Some(v)))
            .collect();
        body.addArrangedSubview(&theme::wrap(mtm, &chips, COL_INNER));
        let hint = modes.current.and_then(|c| known.iter().find(|k| k.0 == c)).and_then(|k| k.2);
        if let Some(h) = hint {
            body.addArrangedSubview(&theme::wrapping(mtm, h, 12.0, &theme::secondary(), COL_INNER));
        }
        self.card(mtm, &body, COL)
    }

    fn sound_card(&self, index: usize, d: &DisplayState, width: f64) -> Retained<NSView> {
        let mtm = self.mtm();
        let inner = width - 2.0 * theme::S2;
        let body = theme::stack(mtm, true, theme::S2);
        let title = theme::label(mtm, t("card.sound"), 14.0, true, &theme::text());
        let head = if d.panel.mute {
            let b = self.button(t(if d.muted { "unmute" } else { "mute" }), sel!(panelMute:));
            b.setTag(index as isize);
            let chip = theme::chip(mtm, &b, d.muted);
            theme::spread(mtm, &[&title], &[&chip])
        } else {
            theme::spread(mtm, &[&title], &[])
        };
        theme::fixed_width(&head, inner);
        body.addArrangedSubview(&head);
        if let Some(v) = d.volume {
            let row = self.level(t("volume"), v, slider_tag(index, Control::Volume.vcp()), sel!(panelControl:), &theme::display_color(index), inner, false);
            if d.muted {
                row.setAlphaValue(0.4);
            }
            body.addArrangedSubview(&row);
        }
        self.card(mtm, &body, width)
    }

    fn color_card(&self, index: usize, d: &DisplayState) -> Retained<NSView> {
        let mtm = self.mtm();
        let p = &d.panel;
        let body = self.card_body(t("card.color"));
        if let Some(presets) = &p.color_preset {
            let chips: Vec<Retained<NSView>> = presets
                .values
                .iter()
                .map(|&v| self.chip(&i18n::color_preset(v), choice_tag(index, presets.code, v), presets.current == Some(v)))
                .collect();
            body.addArrangedSubview(&theme::wrap(mtm, &chips, COL_INNER));
        }
        if p.rgb_active() {
            let gains = [
                (Control::Red, "red", 0),
                (Control::Green, "green", 1),
                (Control::Blue, "blue", 2),
            ];
            for (c, key, color) in gains {
                if let Some(v) = d.level(c) {
                    body.addArrangedSubview(&self.gain(t(key), v, slider_tag(index, c.vcp()), &theme::gain(color)));
                }
            }
        } else if p.rgb && p.color_preset.as_ref().is_some_and(|c| c.values.contains(&PRESET_USER_1)) {
            body.addArrangedSubview(&theme::wrapping(mtm, t("color.custom_hint"), 12.0, &theme::secondary(), COL_INNER));
        }
        self.card(mtm, &body, COL)
    }

    fn input_card(&self, index: usize, input: &Choice, d_vendor: u32) -> Retained<NSView> {
        let mtm = self.mtm();
        let body = self.card_body(t("card.input"));
        let chips: Vec<Retained<NSView>> = input
            .values
            .iter()
            .map(|&v| {
                let vendor = dusk_core::quirks::input_label(d_vendor, v);
                let mut name = vendor.map_or_else(|| i18n::input(v, &input.values), String::from);
                if input.current == Some(v) {
                    name = format!("{name} · {}", t("this_mac"));
                }
                self.chip(&name, choice_tag(index, input.code, v), input.current == Some(v))
            })
            .collect();
        body.addArrangedSubview(&theme::wrap(mtm, &chips, COL_INNER));
        let hint = if input.current.is_some() { t("input.here") } else { t("input.unknown") };
        body.addArrangedSubview(&theme::wrapping(mtm, hint, 12.0, &theme::secondary(), COL_INNER));
        self.card(mtm, &body, COL)
    }

    fn more_card(&self, index: usize, d: &DisplayState, can_sleep: bool) -> Retained<NSView> {
        let mtm = self.mtm();
        let p = &d.panel;
        let body = theme::stack(mtm, true, theme::S2);
        let mut rows: Vec<Retained<NSView>> = Vec::new();

        if let Some(v) = d.level(Control::Sharpness).filter(|_| p.sharpness) {
            let color = theme::display_color(index);
            let row = self.level(t("sharpness"), v, slider_tag(index, Control::Sharpness.vcp()), sel!(panelControl:), &color, COL_INNER, false);
            rows.push(Retained::into_super(row));
        }
        if let Some(lang) = &p.osd_language {
            let popup = NSPopUpButton::initWithFrame_pullsDown(
                NSPopUpButton::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 24.0)),
                false,
            );
            for &v in &lang.values {
                popup.addItemWithTitle(&ns(&i18n::osd_language(v)));
                if let Some(item) = popup.lastItem() {
                    item.setTag(v as isize);
                }
            }
            match lang.current {
                Some(v) => {
                    popup.selectItemWithTag(v as isize);
                }
                None => popup.selectItem(None),
            }
            popup.setTag(index as isize);
            unsafe {
                popup.setTarget(Some(self.target()));
                popup.setAction(Some(sel!(panelLanguage:)));
            }
            theme::fixed_width(&popup, 200.0);
            let label = theme::label(mtm, t("osd_language"), 13.0, true, &theme::text());
            rows.push(Retained::into_super(theme::spread(mtm, &[&label], &[&popup])));
        }
        if can_sleep {
            let b = self.bordered_button(t("sleep.btn"), sel!(panelSleep:));
            b.setTag(index as isize);
            rows.push(self.action_row(t("sleep"), t("sleep.hint"), &b));
        }
        if p.factory_reset {
            let b = self.bordered_button(t("reset.btn"), sel!(panelReset:));
            b.setTag(index as isize);
            rows.push(self.action_row(t("reset"), t("reset.hint"), &b));
        }
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                let sep = theme::separator(mtm);
                theme::fixed_width(&sep, FULL_INNER);
                body.addArrangedSubview(&sep);
            }
            theme::fixed_width(row, FULL_INNER);
            body.addArrangedSubview(row);
        }
        self.card(mtm, &body, CONTENT)
    }

    /// Écran sans canal matériel : pourquoi il n'y a que la luminosité.
    fn software_card(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let icon = objc2_app_kit::NSImageView::imageViewWithImage(&theme::icon(theme::Icon::Tv, 20.0), mtm);
        icon.setContentTintColor(Some(&theme::accent()));
        let tile = theme::card(mtm, &icon, &theme::track(), theme::S1 * 1.5, theme::S1 + theme::HALF);
        theme::fixed_width(&tile, theme::S4 + theme::S1);
        theme::fixed_height(&tile, theme::S4 + theme::S1);
        let text = theme::stack(mtm, true, theme::S1);
        text.addArrangedSubview(&theme::label(mtm, t("soft.title"), 14.0, true, &theme::text()));
        let room = COL_INNER - (theme::S4 + theme::S1) - theme::S2;
        text.addArrangedSubview(&theme::wrapping(mtm, t("soft.desc"), 13.0, &theme::secondary(), room));
        let row = theme::stack(mtm, false, theme::S2);
        row.setAlignment(objc2_app_kit::NSLayoutAttribute::Top);
        row.addArrangedSubview(&tile);
        row.addArrangedSubview(&text);
        self.card(mtm, &row, COL)
    }

    // --- Éléments ---------------------------------------------------------------

    fn card_body(&self, title: &str) -> Retained<NSStackView> {
        let mtm = self.mtm();
        let body = theme::stack(mtm, true, theme::S2);
        body.addArrangedSubview(&theme::label(mtm, title, 14.0, true, &theme::text()));
        body
    }

    fn card(&self, mtm: MainThreadMarker, body: &NSView, width: f64) -> Retained<NSView> {
        let card = theme::card(mtm, body, &theme::card_bg(), theme::S2, theme::S2);
        theme::fixed_width(&card, width);
        Retained::into_super(card)
    }

    fn chip(&self, title: &str, tag: isize, selected: bool) -> Retained<NSView> {
        let b = self.button(title, sel!(panelChoice:));
        b.setTag(tag);
        Retained::into_super(theme::chip(self.mtm(), &b, selected))
    }

    /// Curseur avec son nom et sa valeur au-dessus.
    /// `dim` : curseur de luminosité avec la zone « Plus sombre » sous zéro.
    fn level(&self, name: &str, value: f32, tag: isize, action: Sel, color: &objc2_app_kit::NSColor, width: f64, dim: bool) -> Retained<NSStackView> {
        let mtm = self.mtm();
        let label = theme::label(mtm, name, 13.0, false, &theme::secondary());
        let is_brightness = tag % 256 == 0x10;
        let text = if is_brightness { crate::i18n::level(value) } else { pct(value) };
        let v = theme::value_label(mtm, &text, 13.0);
        let head = theme::spread(mtm, &[&label], &[&v]);
        theme::fixed_width(&head, width);
        let s = self.slider(value, tag, action, color);
        if dim {
            theme::dim_range(&s);
        }
        theme::fixed_width(&s, width);
        self.ivars().labels.borrow_mut().insert(tag, v);
        let row = theme::stack(mtm, true, theme::S1);
        row.addArrangedSubview(&head);
        row.addArrangedSubview(&s);
        row
    }

    /// Curseur compact sur une ligne (rouge, vert, bleu).
    fn gain(&self, name: &str, value: f32, tag: isize, color: &objc2_app_kit::NSColor) -> Retained<NSView> {
        let mtm = self.mtm();
        let label = theme::label(mtm, name, 13.0, false, &theme::secondary());
        theme::fixed_width(&label, theme::S6 + theme::S2);
        let v = theme::value_label(mtm, &format!("{value:.0}"), 13.0);
        v.setAlignment(objc2_app_kit::NSTextAlignment::Right);
        theme::fixed_width(&v, theme::S4);
        let s = self.slider(value, tag, sel!(panelControl:), color);
        theme::fixed_width(&s, COL_INNER - theme::S6 - theme::S2 - theme::S4 - 2.0 * theme::S1);
        self.ivars().labels.borrow_mut().insert(tag, v.clone());
        let row = theme::stack(mtm, false, theme::S1);
        row.addArrangedSubview(&label);
        row.addArrangedSubview(&s);
        row.addArrangedSubview(&v);
        Retained::into_super(row)
    }

    /// Ligne « titre + explication » avec un bouton à droite.
    fn action_row(&self, title: &str, hint: &str, button: &NSButton) -> Retained<NSView> {
        let mtm = self.mtm();
        let text = theme::stack(mtm, true, theme::HALF);
        text.addArrangedSubview(&theme::label(mtm, title, 13.0, true, &theme::text()));
        text.addArrangedSubview(&theme::label(mtm, hint, 12.0, false, &theme::secondary()));
        Retained::into_super(theme::spread(mtm, &[&text], &[button]))
    }

    pub(super) fn slider(&self, value: f32, tag: isize, action: Sel, fill: &objc2_app_kit::NSColor) -> Retained<NSSlider> {
        let s = unsafe {
            NSSlider::sliderWithValue_minValue_maxValue_target_action(value as f64, 0.0, 100.0, Some(self.target()), Some(action), self.mtm())
        };
        s.setTag(tag);
        s.setContinuous(true);
        s.setTrackFillColor(Some(fill));
        s
    }
}

/// Un réglage de l'écran a-t-il bougé depuis `before` ?
pub fn changed(before: &DisplayState, now: &DisplayState) -> bool {
    let choices = |d: &DisplayState| {
        [&d.panel.picture_mode, &d.panel.color_preset, &d.panel.osd_language].map(|c| c.as_ref().and_then(|c| c.current))
    };
    (before.brightness - now.brightness).abs() > 0.5
        || before.levels.iter().any(|(k, v)| now.levels.get(k).is_some_and(|n| (n - v).abs() > 0.5))
        || choices(before) != choices(now)
        || before.muted != now.muted
}

/// Modes d'image connus : (valeur, nom, phrase d'usage), dans l'ordre d'affichage.
/// Un mode que Dusk ne sait pas nommer n'est pas proposé.
fn known_modes(modes: &Choice, vendor: u32) -> Vec<(u8, &'static str, Option<&'static str>)> {
    if modes.code == VCP_DISPLAY_MODE {
        return modes
            .values
            .iter()
            .filter_map(|&v| i18n::display_mode(v).map(|n| (v, n, i18n::display_mode_hint(v))))
            .collect();
    }
    if modes.code == caps::VCP_LG_PICTURE_MODE {
        return dusk_core::quirks::for_vendor(vendor)
            .modes
            .iter()
            .filter(|(v, _)| modes.values.contains(v))
            .map(|&(v, key)| (v, t(key), i18n::hint(key)))
            .collect();
    }
    Vec::new()
}
