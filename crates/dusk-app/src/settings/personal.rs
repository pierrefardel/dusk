//! Page Personnalisation : thème, apparence (sombre, clair, système) et langue.

use super::{header, Settings, CONTENT};
use crate::i18n::{self, t, tx, Lang};
use crate::theme::{self, ns};
use objc2::rc::Retained;
use objc2::{sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBox, NSBoxType, NSPopUpButton, NSTitlePosition, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};

/// Apparences proposées, dans l'ordre des boutons.
pub const APPEARANCES: [&str; 3] = ["dark", "light", "system"];
/// Langues proposées : automatique, puis chaque langue.
pub const LANGUAGES: [&str; 3] = ["auto", "fr", "en"];

const CARD_INNER: f64 = CONTENT - 2.0 * theme::S2;
const TILE: f64 = (CARD_INNER - 2.0 * theme::S2) / 3.0;
const TILE_INNER: f64 = TILE - 2.0 * theme::S2;
const HALF: f64 = TILE_INNER / 2.0;

impl Settings {
    pub(super) fn page_personal(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let config = self.ivars().engine.config();
        let page = header(mtm, t("nav.personal"), t("pers.subtitle"));

        // Thème : une tuile par thème, avec son aperçu sombre et clair.
        let body = theme::stack(mtm, true, theme::S2);
        body.addArrangedSubview(&theme::label(mtm, t("pers.theme"), 14.0, true, &theme::text()));
        let row = theme::stack(mtm, false, theme::S2);
        row.setAlignment(objc2_app_kit::NSLayoutAttribute::Top);
        for (i, id) in theme::THEMES.iter().enumerate() {
            row.addArrangedSubview(&self.theme_tile(mtm, i, id, config.theme == *id));
        }
        body.addArrangedSubview(&row);
        page.addArrangedSubview(&self.wide_card(mtm, &body));

        // Apparence.
        let current = APPEARANCES.iter().position(|a| *a == config.appearance).unwrap_or(0);
        let seg = theme::stack(mtm, false, theme::HALF);
        for (i, key) in ["app.dark", "app.light", "app.system"].iter().enumerate() {
            let b = self.button(t(key), sel!(pickAppearance:));
            b.setTag(i as isize);
            seg.addArrangedSubview(&theme::chip(mtm, &b, i == current));
        }
        let seg_box = theme::card(mtm, &seg, &theme::surface(), theme::S2, theme::HALF);
        let hint = if current == 2 { t("pers.app.system_hint") } else { t("pers.app.fixed_hint") };
        let text = self.title_and_hint(mtm, t("pers.appearance"), hint);
        let line = theme::spread(mtm, &[&text], &[&seg_box]);
        theme::fixed_width(&line, CARD_INNER);
        page.addArrangedSubview(&self.wide_card(mtm, &line));

        // Langue.
        let popup = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 24.0)),
            false,
        );
        let names = [
            tx("lang.auto", i18n::lang_name(i18n::system_lang())),
            i18n::lang_name(Lang::Fr).to_string(),
            i18n::lang_name(Lang::En).to_string(),
        ];
        for name in &names {
            popup.addItemWithTitle(&ns(name));
        }
        popup.selectItemAtIndex(LANGUAGES.iter().position(|l| *l == config.language).unwrap_or(0) as isize);
        unsafe {
            popup.setTarget(Some(self.target()));
            popup.setAction(Some(sel!(pickLanguage:)));
        }
        theme::fixed_width(&popup, 200.0);
        let text = self.title_and_hint(mtm, t("pers.language"), t("pers.language.hint"));
        let line = theme::spread(mtm, &[&text], &[&popup]);
        theme::fixed_width(&line, CARD_INNER);
        page.addArrangedSubview(&self.wide_card(mtm, &line));
        Retained::into_super(page)
    }

    fn theme_tile(&self, mtm: MainThreadMarker, index: usize, id: &str, selected: bool) -> Retained<NSView> {
        let [dark, light] = theme::preview(id);
        let preview = theme::stack(mtm, false, 0.0);
        preview.addArrangedSubview(&half(mtm, dark));
        preview.addArrangedSubview(&half(mtm, light));

        let body = theme::stack(mtm, true, theme::S2);
        body.addArrangedSubview(&preview);
        let names = theme::stack(mtm, true, theme::HALF);
        names.addArrangedSubview(&theme::label(mtm, t(&format!("theme.{id}")), 13.0, true, &theme::text()));
        names.addArrangedSubview(&theme::wrapping(mtm, t(&format!("theme.{id}.sub")), 12.0, &theme::secondary(), TILE_INNER));
        body.addArrangedSubview(&names);

        let tile = theme::card(mtm, &body, &theme::surface(), theme::S2, theme::S2);
        tile.setBorderWidth(2.0);
        tile.setBorderColor(&*if selected { theme::accent() } else { theme::surface() });
        theme::fixed_width(&tile, TILE);
        // Toute la tuile est cliquable : bouton transparent posé par-dessus.
        let hit = self.button("", sel!(pickTheme:));
        hit.setTag(index as isize);
        hit.setTransparent(true);
        tile.addSubview(&hit);
        theme::pin(&tile, &hit, 0.0);
        Retained::into_super(tile)
    }

    pub(super) fn title_and_hint(&self, mtm: MainThreadMarker, title: &str, hint: &str) -> Retained<NSView> {
        let s = theme::stack(mtm, true, theme::HALF);
        s.addArrangedSubview(&theme::label(mtm, title, 14.0, true, &theme::text()));
        s.addArrangedSubview(&theme::label(mtm, hint, 12.0, false, &theme::secondary()));
        Retained::into_super(s)
    }

    pub(super) fn wide_card(&self, mtm: MainThreadMarker, body: &NSView) -> Retained<NSView> {
        let card = theme::card(mtm, body, &theme::card_bg(), theme::S2, theme::S2);
        theme::fixed_width(&card, CONTENT);
        Retained::into_super(card)
    }
}

/// Moitié d'aperçu : fond, curseur (action + piste) et une carte.
fn half(mtm: MainThreadMarker, [bg, card, track, accent]: [u32; 4]) -> Retained<NSView> {
    let inner = HALF - 2.0 * theme::S1;
    let bar = theme::stack(mtm, false, 0.0);
    bar.addArrangedSubview(&swatch(mtm, accent, inner * 0.6, theme::S1));
    bar.addArrangedSubview(&swatch(mtm, track, inner * 0.4, theme::S1));
    let s = theme::stack(mtm, true, theme::S1);
    s.addArrangedSubview(&bar);
    s.addArrangedSubview(&swatch(mtm, card, inner, theme::S3));
    let b = theme::card_xy(mtm, &s, &theme::rgba(bg, 1.0), theme::S1, theme::S1, theme::S2);
    theme::fixed_width(&b, HALF);
    Retained::into_super(b)
}

fn swatch(mtm: MainThreadMarker, color: u32, w: f64, h: f64) -> Retained<NSBox> {
    let b = NSBox::new(mtm);
    b.setBoxType(NSBoxType::Custom);
    b.setTitlePosition(NSTitlePosition::NoTitle);
    b.setBorderWidth(0.0);
    b.setCornerRadius(theme::HALF);
    b.setFillColor(&theme::rgba(color, 1.0));
    b.widthAnchor().constraintEqualToConstant(w).setActive(true);
    b.heightAnchor().constraintEqualToConstant(h).setActive(true);
    b
}
