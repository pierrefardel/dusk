//! Moteur partagé par l'app : état des écrans, synchro et écritures asynchrones.
//!
//! Chaque écran a un thread d'écriture qui ne garde que la dernière valeur
//! demandée par réglage : un slider déplacé rapidement n'empile pas des dizaines
//! de trames DDC.
//!
//! Écrans « écriture seule » (ex. HDMI intégré du Mac, qui ne renvoie pas les
//! lectures DDC) : la dernière valeur envoyée est mémorisée dans la config.

use crate::config::Config;
use crate::ddc::{parse_vcp_list, VCP_BRIGHTNESS, VCP_CONTRAST, VCP_INPUT_SOURCE, VCP_VOLUME};
use crate::ambient;
use crate::dim::{self, DIM_MIN};
use crate::display::{list_displays, Display};
use crate::panel::{self, Panel};
use crate::quirks;
use crate::solar::{self, Place, SunTimes};
use crate::sync::Curve;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

#[derive(Debug, Clone)]
pub struct DisplayState {
    /// Identifiant CoreGraphics (valable pendant la session).
    pub id: u32,
    pub key: String,
    pub name: String,
    pub vendor: u32,
    pub backend: &'static str,
    /// Aucun canal matériel : Dusk assombrit l'image (luminosité seulement).
    pub software: bool,
    pub write_only: bool,
    pub brightness: f32,
    /// `None` : réglage absent (ou non activé pour un écran qui ne répond pas).
    pub contrast: Option<f32>,
    pub volume: Option<f32>,
    /// Tous les réglages continus disponibles, en %.
    pub levels: BTreeMap<u8, f32>,
    /// Réglages annoncés par l'écran au-delà des curseurs.
    pub panel: Panel,
    pub muted: bool,
}

/// Écran éteint depuis Dusk (déconnecté côté macOS) : il n'apparaît plus dans la liste.
#[derive(Debug, Clone)]
pub struct SleepingDisplay {
    pub id: u32,
    pub name: String,
}

impl DisplayState {
    pub fn level(&self, c: Control) -> Option<f32> {
        self.levels.get(&c.vcp()).copied()
    }
}

/// Réglages continus, en plus de la luminosité.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Control {
    Contrast,
    Volume,
    Sharpness,
    Red,
    Green,
    Blue,
}

impl Control {
    pub const ALL: [Control; 6] =
        [Control::Contrast, Control::Volume, Control::Sharpness, Control::Red, Control::Green, Control::Blue];

    pub fn vcp(self) -> u8 {
        match self {
            Control::Contrast => VCP_CONTRAST,
            Control::Volume => VCP_VOLUME,
            Control::Sharpness => panel::VCP_SHARPNESS,
            Control::Red => panel::VCP_RED_GAIN,
            Control::Green => panel::VCP_GREEN_GAIN,
            Control::Blue => panel::VCP_BLUE_GAIN,
        }
    }

    pub fn from_vcp(code: u8) -> Option<Control> {
        Control::ALL.into_iter().find(|c| c.vcp() == code)
    }
}

/// Pause après un réglage à valeurs listées (mode d'image, couleur, entrée).
const SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

/// Écriture en attente : pourcentage (mis à l'échelle du max) ou valeur brute.
#[derive(Debug, Clone, Copy)]
enum Write {
    Pct(f32),
    Raw(u16),
    /// Valeur brute envoyée depuis l'adresse source alternative (exceptions fabricant).
    Alt(u16),
}

/// Dernière valeur demandée par code VCP, consommée par le thread d'écriture.
struct Slot {
    pending: Mutex<BTreeMap<u8, Write>>,
    cv: Condvar,
    stopped: AtomicBool,
    /// Écran qui répond : chaque réglage listé est relu pour vérifier qu'il a pris.
    verify: bool,
    /// Réglages que l'écran a refusés deux fois de suite (à masquer).
    refused: Mutex<Vec<u8>>,
}

impl Slot {
    fn push(&self, v: f32) {
        self.push_vcp(VCP_BRIGHTNESS, v);
    }

    fn push_vcp(&self, code: u8, v: f32) {
        self.push_write(code, Write::Pct(v));
    }

    fn push_raw(&self, code: u8, v: u16) {
        self.push_write(code, Write::Raw(v));
    }

    fn push_write(&self, code: u8, w: Write) {
        self.pending.lock().unwrap().insert(code, w);
        self.cv.notify_one();
    }

    /// Arrête le thread d'écriture (écran débranché ou nouvelle détection).
    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        let _g = self.pending.lock().unwrap();
        self.cv.notify_one();
    }
}

struct Entry {
    display: Arc<Display>,
    key: String,
    brightness: f32,
    write_only: bool,
    controls: HashMap<u8, f32>,
    panel: Panel,
    muted: bool,
    slot: Arc<Slot>,
}

impl Entry {
    fn apply(&mut self, v: f32, config: &mut Config) {
        // Sans canal matériel, tout le curseur assombrit l'image : pas de zone sous zéro.
        let min = if self.display.has_hardware() { DIM_MIN } else { 0.0 };
        self.brightness = v.clamp(min, 100.0);
        self.push_level();
        config.last.insert(self.key.clone(), self.brightness);
    }

    /// Envoie la luminosité : l'écran reçoit au moins 0, la part sous zéro assombrit l'image.
    fn push_level(&self) {
        let software = !self.display.has_hardware();
        if !software {
            self.slot.push(self.brightness.max(0.0));
        }
        dim::apply(self.display.id, self.brightness, software);
    }
}

pub struct Engine {
    inner: Mutex<Inner>,
}

struct Inner {
    entries: Vec<Entry>,
    config: Config,
    sleeping: Vec<SleepingDisplay>,
    /// Lieu du fuseau horaire, pour le cycle solaire.
    place: Place,
    /// Réglage manuel pendant le cycle solaire : il est gardé jusqu'à cet instant (Unix).
    held_until: i64,
    /// Les courbes du curseur commun reflètent l'écart actuel des écrans ; `false` dès
    /// qu'un écran est réglé seul (l'écart sera repris au prochain geste commun).
    anchored: bool,
    /// Lumière de la pièce (lux, lissée) ; `None` sans capteur.
    lux: Option<f64>,
    /// Réglage manuel pendant le suivi de la pièce : gardé tant que la lumière ne change
    /// pas nettement (position pièce sombre / claire au moment du réglage).
    room_held: Option<f32>,
}

/// Moment de la journée vu par « Suivre le soleil ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolarPhase {
    Day,
    Night,
    /// Passage du jour à la nuit, ou l'inverse, en cours.
    Moving,
}

/// État de « Suivre le soleil », pour l'affichage.
#[derive(Debug, Clone)]
pub struct SolarStatus {
    pub enabled: bool,
    /// Ville du fuseau horaire (vide si inconnue).
    pub city: String,
    /// `None` : nuit ou jour polaire, rien n'est appliqué.
    pub sun: Option<SunTimes>,
    pub transition: f32,
    /// Minute locale actuelle.
    pub minute: f64,
    pub factor: f32,
    pub phase: SolarPhase,
    /// Minute locale (modulo 24 h) du prochain passage.
    pub next_change: f64,
    /// Pause en cours : minute locale de reprise.
    pub paused_until: Option<f64>,
    /// Réglage manuel gardé : minute locale de reprise.
    pub held_until: Option<f64>,
    /// Le Mac a un capteur de lumière.
    pub room_available: bool,
    pub room_enabled: bool,
    /// Position entre pièce sombre (0) et claire (1), quand un capteur mesure.
    pub room: Option<f32>,
}

/// Écart maximal appliqué par le cycle solaire à chaque pas (toutes les 30 s).
const SOLAR_MAX_STEP: f32 = 4.0;
/// Lumière de la pièce : pas plus fins mais fréquents (toutes les 2 s).
const ROOM_MAX_STEP: f32 = 2.0;
/// Écart de lumière (position sombre / claire) qui libère un réglage manuel.
const ROOM_RELEASE: f32 = 0.3;

/// Valeurs de départ (jour, nuit) d'après la luminosité actuelle.
fn solar_init(current: f32, factor: f32) -> (f32, f32) {
    if factor >= 0.5 {
        (current, (current * 0.6).round())
    } else {
        ((current / 0.6).min(100.0).round(), current)
    }
}

/// Minute locale correspondant à un instant Unix proche.
fn local_minute(now: &solar::Now, unix: i64) -> f64 {
    (now.minute + (unix - now.unix) as f64 / 60.0).rem_euclid(1440.0)
}

impl Inner {
    /// Position pièce sombre / claire, si la pièce est suivie et qu'un capteur mesure.
    fn room_factor(&self) -> Option<f32> {
        self.config.room.then_some(self.lux).flatten().map(ambient::factor)
    }

    fn sun(&self, now: &solar::Now) -> Option<SunTimes> {
        solar::sun_times(self.place.lat, self.place.lon, now.yday, now.offset)
    }

    /// Réglage manuel de la luminosité (`key` vide : curseur commun) : appris
    /// comme valeur de jour ou de nuit, et gardé jusqu'au prochain passage.
    fn solar_manual(&mut self, key: &str, v: f32) {
        if !self.config.solar.enabled {
            return;
        }
        if let Some(f) = self.room_factor() {
            // Pièce : on déplace la valeur (claire ou sombre) la plus proche pour que
            // le mélange donne exactement ce réglage à ce niveau de lumière.
            let lv = self.config.solar.levels.entry(key.to_string()).or_insert_with(|| solar_init(v, f));
            if f >= 0.5 {
                lv.0 = ((v - lv.1 * (1.0 - f)) / f).clamp(DIM_MIN, 100.0);
            } else {
                lv.1 = ((v - lv.0 * f) / (1.0 - f)).clamp(DIM_MIN, 100.0);
            }
            self.room_held = Some(f);
            return;
        }
        let now = solar::now();
        let Some(sun) = self.sun(&now) else { return };
        let tr = self.config.solar.transition as f64;
        let f = solar::day_factor(now.minute, sun, tr);
        let lv = self.config.solar.levels.entry(key.to_string()).or_insert_with(|| solar_init(v, f));
        if f >= 0.99 {
            lv.0 = v;
        } else if f <= 0.01 {
            lv.1 = v;
        }
        let next = solar::next_transition(now.minute, sun, tr);
        self.held_until = now.unix + ((next - now.minute) * 60.0) as i64;
    }

    /// Rapproche chaque écran de sa luminosité visée. `immediate` : sans limite de pas.
    fn solar_apply(&mut self, immediate: bool) -> bool {
        let now = solar::now();
        if !self.config.solar.enabled || now.unix < self.config.solar.paused_until {
            return false;
        }
        let room = self.room_factor();
        let f = match room {
            Some(f) => {
                // Réglage manuel gardé jusqu'à un net changement de lumière.
                if self.room_held.is_some_and(|h| (h - f).abs() < ROOM_RELEASE) {
                    return false;
                }
                self.room_held = None;
                f
            }
            None => {
                if now.unix < self.held_until {
                    return false;
                }
                let Some(sun) = self.sun(&now) else { return false };
                solar::day_factor(now.minute, sun, self.config.solar.transition as f64)
            }
        };
        let max_step = if room.is_some() { ROOM_MAX_STEP } else { SOLAR_MAX_STEP };
        let step = |cur: f32, target: f32| {
            let d = target - cur;
            if d.abs() < 1.0 {
                None
            } else if immediate {
                Some(target)
            } else {
                Some(cur + d.clamp(-max_step, max_step))
            }
        };
        let Inner { entries, config, .. } = self;
        let mut changed = false;
        for e in entries.iter_mut() {
            if config.solar.excluded.contains(&e.key) {
                continue;
            }
            let (d, n) = *config.solar.levels.entry(e.key.clone()).or_insert_with(|| {
                changed = true;
                solar_init(e.brightness, f)
            });
            if let Some(v) = step(e.brightness, solar::blend(d, n, f)) {
                e.apply(v, config);
                changed = true;
            }
        }
        if changed {
            // Les écrans ont bougé chacun de leur côté : le curseur commun suit le plus lumineux.
            config.master = entries.iter().map(|e| e.brightness).fold(DIM_MIN, f32::max);
            self.anchored = false;
            let _ = config.save();
        }
        changed
    }
}

impl Engine {
    /// Détecte les écrans et lit leurs réglages actuels (bloquant, ~100 ms par
    /// réglage DDC, plus ~1 s la première fois pour lire les capacités).
    pub fn new() -> Arc<Self> {
        let mut config = Config::load();
        let entries = detect(&mut config);
        // Le curseur commun part du plus lumineux des écrans.
        if !entries.is_empty() {
            config.master = entries.iter().map(|e| e.brightness).fold(DIM_MIN, f32::max);
        }
        let place = solar::place(solar::now().offset);
        Arc::new(Engine { inner: Mutex::new(Inner { entries, config, sleeping: Vec::new(), place, held_until: 0, anchored: false, lux: ambient::lux(), room_held: None }) })
    }

    /// Redétecte les écrans (branchement, débranchement, réveil). Bloquant.
    pub fn rescan(&self) {
        let mut config = self.inner.lock().unwrap().config.clone();
        let entries = detect(&mut config);
        let mut g = self.inner.lock().unwrap();
        for old in &g.entries {
            old.slot.stop();
        }
        g.entries = entries;
        if !g.entries.is_empty() {
            g.config.master = g.entries.iter().map(|e| e.brightness).fold(DIM_MIN, f32::max);
        }
        g.anchored = false;
        g.config.caps = config.caps;
        g.place = solar::place(solar::now().offset);
        let _ = g.config.save();
    }

    /// Renvoie à chaque écran sa dernière luminosité et ses curseurs (utile au
    /// réveil : certains écrans repartent sur leur propre valeur).
    pub fn reapply(&self) {
        let g = self.inner.lock().unwrap();
        for e in g.entries.iter() {
            e.push_level();
            for (&code, &v) in &e.controls {
                e.slot.push_vcp(code, v);
            }
        }
    }

    pub fn config(&self) -> Config {
        self.inner.lock().unwrap().config.clone()
    }

    /// Modifie la config et l'enregistre.
    pub fn update_config(&self, f: impl FnOnce(&mut Config)) {
        let mut g = self.inner.lock().unwrap();
        f(&mut g.config);
        let _ = g.config.save();
    }

    pub fn displays(&self) -> Vec<DisplayState> {
        let mut g = self.inner.lock().unwrap();
        let Inner { entries, config, .. } = &mut *g;
        // Réglages refusés depuis le dernier appel : masqués et retenus pour cet écran.
        let mut changed = false;
        for e in entries.iter_mut() {
            let codes = std::mem::take(&mut *e.slot.refused.lock().unwrap());
            if codes.is_empty() {
                continue;
            }
            hide(&mut e.panel, &mut e.controls, &codes);
            let known = config.refused.entry(e.key.clone()).or_default();
            for c in codes {
                if !known.contains(&c) {
                    known.push(c);
                    changed = true;
                }
            }
        }
        if changed {
            let _ = config.save();
        }
        g.entries
            .iter()
            .map(|e| DisplayState {
                id: e.display.id,
                key: e.key.clone(),
                name: e.display.name.clone(),
                vendor: e.display.vendor,
                backend: e.display.backend_name(),
                software: !e.display.has_hardware(),
                write_only: e.write_only,
                brightness: e.brightness,
                contrast: e.controls.get(&VCP_CONTRAST).copied(),
                volume: e.controls.get(&VCP_VOLUME).copied(),
                levels: e.controls.iter().map(|(&k, &v)| (k, v)).collect(),
                panel: e.panel.clone(),
                muted: e.muted,
            })
            .collect()
    }

    pub fn master(&self) -> f32 {
        self.inner.lock().unwrap().config.master
    }

    /// Curseur « Tous les écrans » : chaque écran suit via sa courbe. Au premier geste
    /// après un réglage individuel, l'écart du moment devient la référence.
    pub fn set_master(&self, value: f32) {
        let mut g = self.inner.lock().unwrap();
        let v = value.clamp(DIM_MIN, 100.0);
        if !g.anchored {
            let Inner { entries, config, .. } = &mut *g;
            for e in entries.iter() {
                config.curves.insert(e.key.clone(), Curve::anchored(config.master, e.brightness));
            }
            g.anchored = true;
        }
        let Inner { entries, config, .. } = &mut *g;
        config.master = v;
        let mut moved = Vec::new();
        for e in entries.iter_mut() {
            // Sous zéro, tous les écrans sont au minimum : même assombrissement pour tous.
            let target = if v < 0.0 { v } else { config.curves.get(&e.key).map_or(v, |c| c.eval(v)) };
            e.apply(target, config);
            moved.push((e.key.clone(), e.brightness));
        }
        for (key, b) in moved {
            g.solar_manual(&key, b);
        }
        let _ = g.config.save();
    }

    /// Règle un écran seul. Le curseur commun suit le plus lumineux, et l'écart sera
    /// repris tel quel au prochain geste commun.
    pub fn set_brightness(&self, index: usize, value: f32) {
        let mut g = self.inner.lock().unwrap();
        let Inner { entries, config, .. } = &mut *g;
        let Some(e) = entries.get_mut(index) else { return };
        e.apply(value, config);
        let (key, v) = (e.key.clone(), e.brightness);
        config.master = entries.iter().map(|e| e.brightness).fold(DIM_MIN, f32::max);
        g.anchored = false;
        g.solar_manual(&key, v);
        let _ = g.config.save();
    }

    /// Pas sur tous les écrans (`delta` en points, signé) : via le curseur commun.
    pub fn step_all(&self, delta: f32) {
        let master = self.inner.lock().unwrap().config.master;
        self.set_master(master + delta);
    }

    // --- Suivre le soleil ---------------------------------------------------

    /// Pas du cycle solaire (appelé toutes les 30 s). `true` si une luminosité a changé.
    /// Mesure la lumière de la pièce (toutes les 2 s) et rapproche les écrans si elle est
    /// suivie. `true` si une luminosité a changé.
    pub fn ambient_tick(&self) -> bool {
        let mut g = self.inner.lock().unwrap();
        let Some(raw) = ambient::lux() else {
            g.lux = None;
            return false;
        };
        // Lissage : une main qui passe devant le capteur ne fait pas clignoter les écrans.
        g.lux = Some(g.lux.map_or(raw, |l| l + (raw - l) * 0.3));
        if g.room_factor().is_none() {
            return false;
        }
        g.solar_apply(false)
    }

    pub fn set_room_enabled(&self, on: bool) {
        let mut g = self.inner.lock().unwrap();
        g.config.room = on;
        g.room_held = None;
        let _ = g.config.save();
    }

    pub fn solar_tick(&self) -> bool {
        self.inner.lock().unwrap().solar_apply(false)
    }

    /// Au lancement et au réveil : directement la bonne luminosité, sans descente progressive
    /// (le Mac a pu dormir pendant tout le coucher du soleil).
    pub fn solar_jump(&self) -> bool {
        self.inner.lock().unwrap().solar_apply(true)
    }

    pub fn solar_status(&self) -> SolarStatus {
        let g = self.inner.lock().unwrap();
        let now = solar::now();
        let s = &g.config.solar;
        let sun = g.sun(&now);
        let tr = s.transition as f64;
        let factor = sun.map_or(1.0, |sun| solar::day_factor(now.minute, sun, tr));
        let moving = sun.is_some_and(|sun| solar::in_transition(now.minute, sun, tr));
        let phase = match factor {
            _ if moving => SolarPhase::Moving,
            f if f >= 0.99 => SolarPhase::Day,
            f if f <= 0.01 => SolarPhase::Night,
            _ => SolarPhase::Moving,
        };
        SolarStatus {
            enabled: s.enabled,
            city: g.place.city.clone(),
            sun,
            transition: s.transition,
            minute: now.minute,
            factor,
            phase,
            next_change: sun.map_or(0.0, |sun| solar::next_transition(now.minute, sun, tr).rem_euclid(1440.0)),
            paused_until: (s.paused_until > now.unix).then(|| local_minute(&now, s.paused_until)),
            held_until: (g.held_until > now.unix).then(|| local_minute(&now, g.held_until)),
            room_available: g.lux.is_some(),
            room_enabled: g.config.room,
            room: g.lux.map(ambient::factor),
        }
    }

    pub fn set_solar_enabled(&self, on: bool) {
        let mut g = self.inner.lock().unwrap();
        g.config.solar.enabled = on;
        g.config.solar.paused_until = 0;
        g.held_until = 0;
        let _ = g.config.save();
    }

    /// Durée du passage jour / nuit, en minutes.
    pub fn set_solar_transition(&self, minutes: f32) {
        let mut g = self.inner.lock().unwrap();
        g.config.solar.transition = minutes;
        let _ = g.config.save();
    }

    /// Valeur de jour (`day`) ou de nuit d'un écran (clé vide : curseur commun),
    /// appliquée tout de suite si c'est le moment.
    pub fn set_solar_level(&self, key: &str, day: bool, value: f32) {
        let mut g = self.inner.lock().unwrap();
        let v = value.clamp(DIM_MIN, 100.0);
        let current = g.entries.iter().find(|e| e.key == key).map_or(50.0, |e| e.brightness);
        let lv = g.config.solar.levels.entry(key.to_string()).or_insert_with(|| solar_init(current, 1.0));
        if day {
            lv.0 = v;
        } else {
            lv.1 = v;
        }
        g.held_until = 0;
        let _ = g.config.save();
        g.solar_apply(true);
    }

    pub fn set_solar_excluded(&self, key: &str, excluded: bool) {
        let mut g = self.inner.lock().unwrap();
        let list = &mut g.config.solar.excluded;
        list.retain(|k| k != key);
        if excluded {
            list.push(key.to_string());
        }
        let _ = g.config.save();
    }

    /// Met le cycle solaire en pause (`seconds` > 0) ou le reprend (0).
    pub fn solar_pause(&self, seconds: i64) {
        let mut g = self.inner.lock().unwrap();
        g.config.solar.paused_until = if seconds > 0 { solar::now().unix + seconds } else { 0 };
        g.held_until = 0;
        let _ = g.config.save();
    }

    /// Pas sur un seul écran, identifié par son ID CoreGraphics. Renvoie son index.
    pub fn step_display(&self, id: u32, delta: f32) -> Option<usize> {
        let (index, current) = {
            let g = self.inner.lock().unwrap();
            let i = g.entries.iter().position(|e| e.display.id == id)?;
            (i, g.entries[i].brightness)
        };
        self.set_brightness(index, current + delta);
        Some(index)
    }

    /// Pas par défaut des raccourcis, en points.
    pub fn step_size(&self) -> f32 {
        self.inner.lock().unwrap().config.step
    }

    /// Règle un curseur (contraste, volume, netteté, rouge, vert, bleu).
    pub fn set_control(&self, index: usize, control: Control, value: f32) {
        let mut g = self.inner.lock().unwrap();
        let Inner { entries, config, .. } = &mut *g;
        let Some(e) = entries.get_mut(index) else { return };
        let code = control.vcp();
        if !e.controls.contains_key(&code) {
            return;
        }
        let v = value.clamp(0.0, 100.0);
        e.controls.insert(code, v);
        e.slot.push_vcp(code, v);
        config.last_controls.entry(e.key.clone()).or_default().insert(code, v);
        let _ = config.save();
    }

    /// Choisit une valeur listée (mode d'image, couleur, entrée, langue des menus).
    pub fn set_choice(&self, index: usize, code: u8, value: u8) {
        let mut g = self.inner.lock().unwrap();
        let Inner { entries, config, .. } = &mut *g;
        let Some(e) = entries.get_mut(index) else { return };
        let Some(c) = e.panel.choices_mut().find(|c| c.code == code && c.values.contains(&value)) else { return };
        c.current = Some(value);
        let quirk = (code == VCP_INPUT_SOURCE).then(|| quirks::for_vendor(e.display.vendor).input).flatten();
        match quirk.and_then(|q| q.map.iter().find(|(v, _)| *v == value).map(|&(_, raw)| (q, raw))) {
            Some((q, raw)) => e.slot.push_write(q.code, if q.alt_address { Write::Alt(raw) } else { Write::Raw(raw) }),
            None => e.slot.push_raw(code, value as u16),
        }
        if crate::ddc::debug() {
            eprintln!("[dusk] {} : choix VCP {code:#04x} -> {value:#04x}", e.display.name);
        }
        // L'entrée n'est pas mémorisée : on ne la réimpose jamais.
        if code != VCP_INPUT_SOURCE {
            config.last_choices.entry(e.key.clone()).or_default().insert(code, value);
            let _ = config.save();
        }
    }

    pub fn set_muted(&self, index: usize, on: bool) {
        let mut g = self.inner.lock().unwrap();
        let Inner { entries, config, .. } = &mut *g;
        let Some(e) = entries.get_mut(index).filter(|e| e.panel.mute) else { return };
        e.muted = on;
        let v = if on { panel::MUTE_ON } else { panel::MUTE_OFF };
        e.slot.push_raw(panel::VCP_MUTE, v as u16);
        config.last_choices.entry(e.key.clone()).or_default().insert(panel::VCP_MUTE, v);
        let _ = config.save();
    }

    /// Éteint un écran en le déconnectant côté macOS (refusé s'il est le dernier allumé).
    /// Si Dusk quitte ou plante, macOS le reconnecte de lui-même.
    pub fn sleep_display(&self, index: usize) -> bool {
        let mut g = self.inner.lock().unwrap();
        if g.entries.len() < 2 {
            return false;
        }
        let Some(e) = g.entries.get(index) else { return false };
        let (id, name) = (e.display.id, e.display.name.clone());
        if !crate::cg::set_display_enabled(id, false) {
            return false;
        }
        g.sleeping.push(SleepingDisplay { id, name });
        true
    }

    pub fn wake_display(&self, id: u32) {
        let mut g = self.inner.lock().unwrap();
        crate::cg::set_display_enabled(id, true);
        g.sleeping.retain(|s| s.id != id);
    }

    pub fn sleeping(&self) -> Vec<SleepingDisplay> {
        self.inner.lock().unwrap().sleeping.clone()
    }

    /// Réglages d'usine. Les valeurs retenues pour cet écran sont oubliées ;
    /// une redétection quelques secondes plus tard relit celles de l'écran.
    pub fn factory_reset(&self, index: usize) {
        let mut g = self.inner.lock().unwrap();
        let Inner { entries, config, .. } = &mut *g;
        let Some(e) = entries.get_mut(index).filter(|e| e.panel.factory_reset) else { return };
        e.slot.push_raw(panel::VCP_FACTORY_RESET, 1);
        config.last_controls.remove(&e.key);
        config.last_choices.remove(&e.key);
        if e.write_only {
            for v in e.controls.values_mut() {
                *v = 50.0;
            }
            for c in e.panel.choices_mut() {
                c.current = None;
            }
            e.muted = false;
        }
        let _ = config.save();
    }

    /// Écran qui ne répond pas et n'annonce rien : l'utilisateur indique lui-même
    /// que contraste et volume existent. Les curseurs repartent de la dernière valeur.
    pub fn set_extras_enabled(&self, index: usize, on: bool) {
        let mut g = self.inner.lock().unwrap();
        let Inner { entries, config, .. } = &mut *g;
        let Some(e) = entries.get_mut(index).filter(|e| e.write_only) else { return };
        config.extras.insert(e.key.clone(), on);
        e.controls = if on { remembered_controls(&e.key, config, &[VCP_CONTRAST, VCP_VOLUME]) } else { HashMap::new() };
        let _ = config.save();
    }
}

/// Réglages vérifiables par relecture : pas l'entrée (l'écran bascule), ni les
/// commandes ponctuelles (usine, alimentation).
fn verifiable(code: u8) -> bool {
    !matches!(code, VCP_INPUT_SOURCE | panel::VCP_FACTORY_RESET | panel::VCP_POWER_MODE)
}

/// Masque les réglages refusés par l'écran.
fn hide(panel: &mut Panel, controls: &mut HashMap<u8, f32>, codes: &[u8]) {
    for &code in codes {
        controls.remove(&code);
        for slot in [&mut panel.picture_mode, &mut panel.color_preset, &mut panel.input, &mut panel.osd_language] {
            if slot.as_ref().is_some_and(|c| c.code == code) {
                *slot = None;
            }
        }
        if code == panel::VCP_MUTE {
            panel.mute = false;
        }
    }
}

fn detect(config: &mut Config) -> Vec<Entry> {
    list_displays().into_iter().map(|d| spawn_entry(Arc::new(d), config)).collect()
}

fn remembered_controls(key: &str, config: &Config, codes: &[u8]) -> HashMap<u8, f32> {
    let last = config.last_controls.get(key);
    codes
        .iter()
        .map(|&c| (c, last.and_then(|m| m.get(&c).copied()).unwrap_or(50.0)))
        .collect()
}

/// Capacités de l'écran : en cache dans la config, sinon lues sur le bus.
fn capabilities(display: &Display, key: &str, config: &mut Config) -> BTreeMap<u8, Vec<u8>> {
    if let Some(s) = config.caps.get(key) {
        return parse_vcp_list(s);
    }
    let Some(caps) = display.ddc().and_then(|d| d.capabilities().ok()) else { return BTreeMap::new() };
    let map = parse_vcp_list(&caps);
    if !map.is_empty() {
        config.caps.insert(key.to_string(), caps);
    }
    map
}

fn spawn_entry(display: Arc<Display>, config: &mut Config) -> Entry {
    let key = display.key();
    let (read, max) = read_initial(&display);
    let write_only = display.has_hardware() && read.is_none();
    let last = config.last.get(&key).copied();
    // Un écran lu à 0 alors que Dusk l'avait mis « plus sombre » : on garde ce réglage.
    let brightness = match (read, last) {
        (Some(r), Some(l)) if r <= 0.0 && l < 0.0 => l,
        _ => read.or(last).unwrap_or(if display.has_hardware() { 50.0 } else { 100.0 }),
    };
    let brightness = if display.has_hardware() { brightness } else { brightness.max(0.0) };

    let caps = if display.ddc().is_some() { capabilities(&display, &key, config) } else { BTreeMap::new() };
    let mut panel = Panel::from_caps(&caps, display.vendor);
    if quirks::for_vendor(display.vendor).hide_input {
        panel.input = None;
    }
    // Curseurs annoncés ; sans capacités, contraste et volume sont tentés quand même.
    let announced: Vec<u8> = if !display.has_hardware() {
        Vec::new()
    } else if caps.is_empty() {
        vec![VCP_CONTRAST, VCP_VOLUME]
    } else {
        Control::ALL.iter().map(|c| c.vcp()).filter(|c| caps.contains_key(c)).collect()
    };

    let mut maxes: HashMap<u8, u16> = HashMap::from([(VCP_BRIGHTNESS, max)]);
    let mut controls = HashMap::new();
    let mut muted = false;
    let remembered = config.last_choices.get(&key).cloned().unwrap_or_default();
    match (display.ddc(), write_only) {
        (Some(ddc), false) => {
            for code in Control::ALL.iter().map(|c| c.vcp()).filter(|c| announced.contains(c)) {
                if let Ok(v) = ddc.read(code) {
                    let m = if v.max == 0 { 100 } else { v.max };
                    maxes.insert(code, m);
                    controls.insert(code, v.current as f32 * 100.0 / m as f32);
                }
            }
            for c in panel.choices_mut() {
                c.current = ddc.read(c.code).ok().map(|v| v.current as u8).filter(|v| c.values.contains(v));
            }
            if panel.mute {
                muted = ddc.read(panel::VCP_MUTE).is_ok_and(|v| v.current as u8 == panel::MUTE_ON);
            }
        }
        _ if write_only && panel.known => {
            controls = remembered_controls(&key, config, &announced);
            for c in panel.choices_mut().filter(|c| c.code != VCP_INPUT_SOURCE) {
                c.current = remembered.get(&c.code).copied().filter(|v| c.values.contains(v));
            }
            muted = remembered.get(&panel::VCP_MUTE) == Some(&panel::MUTE_ON);
        }
        _ if write_only && config.extras.get(&key).copied().unwrap_or(false) => {
            controls = remembered_controls(&key, config, &[VCP_CONTRAST, VCP_VOLUME]);
        }
        _ => {}
    }

    // Réglages déjà refusés par cet écran : masqués d'emblée.
    let refused = config.refused.get(&key).cloned().unwrap_or_default();
    hide(&mut panel, &mut controls, &refused);

    let slot = Arc::new(Slot {
        pending: Mutex::new(BTreeMap::new()),
        cv: Condvar::new(),
        stopped: AtomicBool::new(false),
        verify: display.ddc().is_some() && !write_only,
        refused: Mutex::new(Vec::new()),
    });
    if display.has_hardware() {
        let (d, s) = (display.clone(), slot.clone());
        thread::Builder::new()
            .name(format!("dusk-writer-{}", display.id))
            .spawn(move || loop {
                let batch = {
                    let mut guard = s
                        .cv
                        .wait_while(s.pending.lock().unwrap(), |p| p.is_empty() && !s.stopped.load(Ordering::Relaxed))
                        .unwrap();
                    if s.stopped.load(Ordering::Relaxed) {
                        break;
                    }
                    std::mem::take(&mut *guard)
                };
                for (code, w) in batch {
                    let max = maxes.get(&code).copied().unwrap_or(100);
                    let raw = match w {
                        Write::Pct(pct) => (pct.clamp(0.0, 100.0) / 100.0 * max as f32).round() as u16,
                        Write::Raw(v) | Write::Alt(v) => v,
                    };
                    let send = || match (code, w, d.ddc()) {
                        (VCP_BRIGHTNESS, Write::Pct(pct), _) => d.set_brightness(pct, max),
                        (_, Write::Alt(_), Some(ddc)) => ddc.write_alt(code, raw),
                        (_, _, Some(ddc)) => ddc.write(code, raw),
                        _ => Ok(()),
                    };
                    if let Err(e) = send() {
                        eprintln!("[dusk] {} : {e}", d.name);
                    }
                    if !matches!(w, Write::Raw(_) | Write::Alt(_)) {
                        continue;
                    }
                    // Mode, couleur, entrée : l'écran met du temps à basculer et
                    // ignore ce qu'il reçoit entre-temps.
                    thread::sleep(SETTLE);
                    // Vérification par relecture (réglages listés, écran qui répond) :
                    // un second refus masque le réglage pour cet écran.
                    let (Some(ddc), true, Write::Raw(_)) = (d.ddc(), s.verify && verifiable(code), w) else { continue };
                    let applied = || ddc.read(code).map(|v| v.current == raw);
                    if applied().is_ok_and(|ok| !ok) {
                        let _ = send();
                        thread::sleep(SETTLE);
                        if applied().is_ok_and(|ok| !ok) {
                            if crate::ddc::debug() {
                                eprintln!("[dusk] {} : VCP {code:#04x} refusé deux fois, réglage masqué", d.name);
                            }
                            s.refused.lock().unwrap().push(code);
                        }
                    }
                }
            })
            .expect("thread d'écriture");
    }
    dim::apply(display.id, brightness, !display.has_hardware());
    Entry { display, key, brightness, write_only, controls, panel, muted, slot }
}

/// Lit la luminosité actuelle et la valeur VCP max. `None` si l'écran ne répond pas.
fn read_initial(d: &Display) -> (Option<f32>, u16) {
    if let Some(s) = d.ddc() {
        return match s.read(VCP_BRIGHTNESS) {
            Ok(v) => {
                let max = if v.max == 0 { 100 } else { v.max };
                (Some(v.current as f32 * 100.0 / max as f32), max)
            }
            Err(_) => (None, 100),
        };
    }
    (d.brightness().ok(), 100)
}
