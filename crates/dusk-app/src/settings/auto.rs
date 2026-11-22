//! Page Automatique : « Suivre le soleil » (vue d'ensemble et courbe détaillée).

use super::{header, Settings, CONTENT};
use crate::i18n::{t, tx};
use crate::sun_view::{SunCurve, SunView};
use crate::theme::{self, ns};
use dusk_core::solar::SunTimes;
use dusk_core::{DisplayState, SolarStatus};
use objc2::rc::Retained;
use objc2::{sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSControlStateValueOff, NSControlStateValueOn, NSImageView, NSSlider, NSSwitch, NSView};
use objc2_foundation::NSSize;

const CARD_INNER: f64 = CONTENT - 2.0 * theme::S2;
/// Durées de passage proposées (minutes) et leurs libellés.
pub const SPEEDS: [(&str, f32); 3] = [("solar.speed.fast", 15.0), ("solar.speed.soft", 40.0), ("solar.speed.slow", 90.0)];
/// Étiquettes des curseurs jour / nuit : à part de celles du panneau de l'écran.
pub const LEVEL_TAG: isize = 100_000;

/// Étiquette d'un curseur jour / nuit : `who` 0 = curseur commun, n = écran n - 1.
pub fn level_tag(who: usize, day: bool) -> isize {
    LEVEL_TAG + (who * 2 + day as usize) as isize
}

pub fn decode_level_tag(tag: isize) -> (Option<usize>, bool) {
    let n = (tag - LEVEL_TAG) as usize;
    let who = n / 2;
    ((who > 0).then(|| who - 1), n % 2 == 1)
}

pub fn hhmm(minute: f64) -> String {
    let m = minute.rem_euclid(1440.0).round() as u32 % 1440;
    format!("{:02}:{:02}", m / 60, m % 60)
}

impl Settings {
    pub(super) fn page_auto(&self) -> Retained<NSView> {
        if self.ivars().solar_detail.get() {
            return self.page_solar();
        }
        let mtm = self.mtm();
        let engine = &self.ivars().engine;
        let st = engine.solar_status();
        let page = header(mtm, t("nav.auto"), t("auto.subtitle"));
        // Lumière de la pièce : seulement si le Mac a un capteur ; elle passe avant le soleil.
        let room = st.room_available && st.room_enabled;
        if st.room_available {
            page.addArrangedSubview(&self.room_card(&st));
        }

        let body = theme::stack(mtm, true, theme::S2);
        let switch = NSSwitch::new(mtm);
        switch.setState(if st.enabled { NSControlStateValueOn } else { NSControlStateValueOff });
        unsafe {
            switch.setTarget(Some(self.target()));
            switch.setAction(Some(sel!(toggleSolar:)));
        }
        let text = theme::stack(mtm, true, theme::HALF);
        text.addArrangedSubview(&theme::label(mtm, t("solar.title"), 14.0, true, &theme::text()));
        let desc = if room && st.enabled { t("solar.paused_room") } else { t("solar.desc") };
        text.addArrangedSubview(&theme::wrapping(mtm, desc, 13.0, &theme::secondary(), CARD_INNER - 136.0));
        let icon = sun_icon(mtm);
        let head = theme::spread(mtm, &[&icon, &text], &[&switch]);
        theme::fixed_width(&head, CARD_INNER);
        body.addArrangedSubview(&head);

        if st.enabled && !room {
            match st.sun {
                Some(sun) => {
                    let (curves, shown) = self.solar_curves(&st);
                    body.addArrangedSubview(&SunView::new(
                        mtm,
                        sun,
                        st.transition as f64,
                        st.minute,
                        curves,
                        NSSize::new(CARD_INNER, theme::S6 * 2.0),
                    ));
                    body.addArrangedSubview(&times_row(mtm, sun, now_text(&st, shown)));
                    let taught = !self.ivars().engine.config().solar.taught.is_empty();
                    let summary = match shown {
                        Some((d, n)) => {
                            let levels =
                                format!("{} · {}", tx("solar.day_short", &pct(d)), tx("solar.night_short", &pct(n)));
                            if taught { format!("{levels} — {}", t("solar.learned")) } else { levels }
                        }
                        None => String::new(),
                    };
                    let summary = theme::label(mtm, &summary, 12.0, false, &theme::secondary());
                    let adjust = self.button(t("solar.adjust"), sel!(solarDetail:));
                    theme::roomy(&adjust);
                    adjust.setContentTintColor(Some(&theme::accent()));
                    let foot = theme::spread(mtm, &[&summary], &[&adjust]);
                    theme::fixed_width(&foot, CARD_INNER);
                    body.addArrangedSubview(&foot);
                }
                None => body.addArrangedSubview(&theme::wrapping(mtm, t("solar.polar"), 12.0, &theme::secondary(), CARD_INNER)),
            }
        }
        page.addArrangedSubview(&self.wide_card(mtm, &body));
        Retained::into_super(page)
    }

    /// Carte « Suivre la lumière de la pièce » (Mac avec capteur).
    fn room_card(&self, st: &SolarStatus) -> Retained<NSView> {
        let mtm = self.mtm();
        let body = theme::stack(mtm, true, theme::S2);
        let switch = NSSwitch::new(mtm);
        switch.setState(if st.room_enabled { NSControlStateValueOn } else { NSControlStateValueOff });
        unsafe {
            switch.setTarget(Some(self.target()));
            switch.setAction(Some(sel!(toggleRoom:)));
        }
        let text = theme::stack(mtm, true, theme::HALF);
        text.addArrangedSubview(&theme::label(mtm, t("room.title"), 14.0, true, &theme::text()));
        text.addArrangedSubview(&theme::wrapping(mtm, t("room.desc"), 13.0, &theme::secondary(), CARD_INNER - 136.0));
        let head = theme::spread(mtm, &[&icon_tile(mtm, theme::Icon::Bulb), &text], &[&switch]);
        theme::fixed_width(&head, CARD_INNER);
        body.addArrangedSubview(&head);

        if st.room_enabled {
            if let Some(f) = st.room {
                let inner_w = CARD_INNER - 2.0 * theme::S2;
                let state = match f {
                    f if f < 0.33 => t("room.state.dark"),
                    f if f < 0.66 => t("room.state.dim"),
                    _ => t("room.state.bright"),
                };
                let now = theme::label(mtm, &tx("room.now", state), 12.0, true, &theme::text());
                let ends = theme::spread(
                    mtm,
                    &[&theme::label(mtm, t("room.dark"), 12.0, false, &theme::secondary())],
                    &[&theme::label(mtm, t("room.bright"), 12.0, false, &theme::secondary())],
                );
                theme::fixed_width(&ends, inner_w);
                let gauge = theme::stack(mtm, true, theme::S1);
                gauge.addArrangedSubview(&now);
                gauge.addArrangedSubview(&theme::pill_bar(mtm, f as f64, inner_w, &theme::spark()));
                gauge.addArrangedSubview(&ends);
                if let (_, Some((d, n))) = self.solar_curves(st) {
                    let screens = tx("room.screens", &pct(dusk_core::solar::blend(d, n, f)));
                    gauge.addArrangedSubview(&theme::label(mtm, &screens, 13.0, true, &theme::accent()));
                }
                let panel = theme::card(mtm, &gauge, &theme::surface(), theme::S1 + theme::HALF, theme::S2);
                theme::fixed_width(&panel, CARD_INNER);
                body.addArrangedSubview(&panel);
            }
            body.addArrangedSubview(&theme::wrapping(mtm, t("room.hint"), 12.0, &theme::secondary(), CARD_INNER));
        }
        self.wide_card(mtm, &body)
    }

    fn page_solar(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let engine = &self.ivars().engine;
        let st = engine.solar_status();
        let config = engine.config();
        let displays: Vec<DisplayState> = engine.displays();

        let page = theme::stack(mtm, true, theme::S2);
        let back = self.button(t("solar.back"), sel!(solarBack:));
        theme::roomy(&back);
        back.setContentTintColor(Some(&theme::accent()));
        page.addArrangedSubview(&back);
        let head = header(mtm, t("solar.title"), t("solar.detail"));
        page.addArrangedSubview(&head);

        // Courbe sur 24 h.
        if let Some(sun) = st.sun {
            let (curves, shown) = self.solar_curves(&st);
            let body = theme::stack(mtm, true, theme::S1);
            let view = SunView::new(mtm, sun, st.transition as f64, st.minute, curves, NSSize::new(CARD_INNER, 160.0));
            body.addArrangedSubview(&view);
            *self.ivars().sun_view.borrow_mut() = Some(view);
            body.addArrangedSubview(&times_row(mtm, sun, now_text(&st, shown)));
            page.addArrangedSubview(&self.wide_card(mtm, &body));
        } else {
            page.addArrangedSubview(&theme::wrapping(mtm, t("solar.polar"), 12.0, &theme::secondary(), CONTENT));
        }

        // Valeurs de jour et de nuit de chaque écran.
        let levels = theme::stack(mtm, true, theme::S2);
        for (i, d) in displays.iter().enumerate() {
            if config.solar.excluded.contains(&d.key) {
                continue;
            }
            let lv = config.solar.levels.get(&d.key).copied().unwrap_or((d.brightness, d.brightness));
            levels.addArrangedSubview(&self.level_pair(mtm, i + 1, Some((i, &d.name)), lv));
        }
        levels.addArrangedSubview(&theme::wrapping(mtm, t("solar.learn_hint"), 12.0, &theme::secondary(), CARD_INNER));
        page.addArrangedSubview(&self.wide_card(mtm, &levels));

        // Vitesse du passage.
        let seg = theme::stack(mtm, false, theme::HALF);
        let current = SPEEDS.iter().position(|s| (s.1 - st.transition).abs() < 0.5).unwrap_or(1);
        for (i, (key, _)) in SPEEDS.iter().enumerate() {
            let b = self.button(t(key), sel!(solarSpeed:));
            b.setTag(i as isize);
            seg.addArrangedSubview(&theme::chip(mtm, &b, i == current));
        }
        let seg_box = theme::card(mtm, &seg, &theme::surface(), theme::S2, theme::HALF);
        let text = self.title_and_hint(mtm, t("solar.speed"), &tx("solar.speed.hint", &format!("{:.0}", st.transition)));
        let line = theme::spread(mtm, &[&text], &[&seg_box]);
        theme::fixed_width(&line, CARD_INNER);
        page.addArrangedSubview(&self.wide_card(mtm, &line));

        // Écrans concernés.
        if displays.len() > 1 {
            let body = theme::stack(mtm, true, theme::S2);
            body.addArrangedSubview(&theme::label(mtm, t("solar.screens"), 14.0, true, &theme::text()));
            let row = theme::stack(mtm, false, theme::S4);
            for d in &displays {
                let index = engine.displays().iter().position(|x| x.key == d.key).unwrap_or(0);
                let b = self.checkbox(&d.name, !config.solar.excluded.contains(&d.key), sel!(solarExclude:));
                b.setTag(index as isize);
                row.addArrangedSubview(&b);
            }
            body.addArrangedSubview(&row);
            page.addArrangedSubview(&self.wide_card(mtm, &body));
        }

        let place = if st.city.is_empty() { t("solar.place_unknown").to_string() } else { tx("solar.place", &st.city) };
        page.addArrangedSubview(&theme::wrapping(mtm, &place, 12.0, &theme::secondary(), CONTENT));
        Retained::into_super(page)
    }

    /// Redessine la courbe détaillée après un changement de valeur jour / nuit.
    pub(super) fn redraw_sun(&self) {
        let st = self.ivars().engine.solar_status();
        let (curves, _) = self.solar_curves(&st);
        if let Some(v) = self.ivars().sun_view.borrow().as_ref() {
            v.set_curves(curves);
        }
    }

    /// Courbes à tracer (curseur commun, ou un écran par couleur), et les valeurs
    /// (jour, nuit) de la première pour le résumé.
    fn solar_curves(&self, _st: &SolarStatus) -> (Vec<SunCurve>, Option<(f32, f32)>) {
        let engine = &self.ivars().engine;
        let config = engine.config();
        let mut curves = Vec::new();
        let mut first = None;
        for (i, d) in engine.displays().iter().enumerate() {
            if config.solar.excluded.contains(&d.key) {
                continue;
            }
            if let Some(&(day, night)) = config.solar.levels.get(&d.key) {
                first.get_or_insert((day, night));
                curves.push(SunCurve { day, night, color: theme::display_color(i) });
            }
        }
        (curves, first)
    }

    /// Deux curseurs côte à côte : jour et nuit.
    fn level_pair(
        &self,
        mtm: MainThreadMarker,
        who: usize,
        name: Option<(usize, &str)>,
        (day, night): (f32, f32),
    ) -> Retained<NSView> {
        let col = (CARD_INNER - theme::S3) / 2.0;
        let row = theme::stack(mtm, false, theme::S3);
        for (is_day, v, key) in [(true, day, "solar.day"), (false, night, "solar.night")] {
            let tag = level_tag(who, is_day);
            let value = theme::value_label(mtm, &pct(v), 13.0);
            self.ivars().labels.borrow_mut().insert(tag, value.clone());
            let title = theme::label(mtm, t(key), 13.0, true, &theme::text());
            let top = theme::spread(mtm, &[&title], &[&value]);
            theme::fixed_width(&top, col);
            let slider = unsafe {
                NSSlider::sliderWithValue_minValue_maxValue_target_action(
                    v as f64,
                    0.0,
                    100.0,
                    Some(self.target()),
                    Some(sel!(solarLevel:)),
                    mtm,
                )
            };
            slider.setTag(tag);
            slider.setContinuous(true);
            slider.setTrackFillColor(Some(&*if is_day { theme::spark() } else { theme::accent() }));
            theme::dim_range(&slider);
            theme::fixed_width(&slider, col);
            let cell = theme::stack(mtm, true, theme::S1);
            cell.addArrangedSubview(&top);
            cell.addArrangedSubview(&slider);
            row.addArrangedSubview(&cell);
        }
        let Some((i, name)) = name else { return Retained::into_super(row) };
        let block = theme::stack(mtm, true, theme::S1);
        let dot = theme::label(mtm, "●", 10.0, false, &theme::display_color(i));
        let n = theme::label(mtm, name, 13.0, true, &theme::text());
        block.addArrangedSubview(&theme::spread(mtm, &[&dot, &n], &[]));
        block.addArrangedSubview(&row);
        Retained::into_super(block)
    }
}

fn sun_icon(mtm: MainThreadMarker) -> Retained<NSView> {
    icon_tile(mtm, theme::Icon::Sun)
}

fn icon_tile(mtm: MainThreadMarker, icon: theme::Icon) -> Retained<NSView> {
    let v = NSImageView::imageViewWithImage(&theme::icon(icon, 20.0), mtm);
    v.setContentTintColor(Some(&theme::spark()));
    let inner: Retained<NSView> = Retained::into_super(Retained::into_super(v));
    let tile = theme::card(mtm, &inner, &theme::track(), theme::S1 * 1.5, theme::S1 + theme::HALF);
    theme::fixed_width(&tile, theme::S4 + theme::S1);
    theme::fixed_height(&tile, theme::S4 + theme::S1);
    Retained::into_super(tile)
}

/// « Lever 08:08 · Coucher 17:04 » à gauche, l'instant présent à droite.
fn times_row(mtm: MainThreadMarker, sun: SunTimes, now: String) -> Retained<NSView> {
    let times = format!("{} · {}", tx("solar.sunrise", &hhmm(sun.sunrise)), tx("solar.sunset", &hhmm(sun.sunset)));
    let left = theme::label(mtm, &times, 12.0, false, &theme::secondary());
    let right = theme::label(mtm, &now, 12.0, true, &theme::text());
    let row = theme::spread(mtm, &[&left], &[&right]);
    theme::fixed_width(&row, CARD_INNER);
    Retained::into_super(row)
}

fn now_text(st: &SolarStatus, shown: Option<(f32, f32)>) -> String {
    match shown {
        Some((d, n)) => tx("solar.now", &pct(dusk_core::solar::blend(d, n, st.factor))),
        None => tx("solar.now", &hhmm(st.minute)),
    }
}

fn pct(v: f32) -> String {
    crate::i18n::level(v)
}

pub fn set_tag_label(s: &Settings, tag: isize, v: f32) {
    if let Some(l) = s.ivars().labels.borrow().get(&tag) {
        l.setStringValue(&ns(&pct(v)));
    }
}
