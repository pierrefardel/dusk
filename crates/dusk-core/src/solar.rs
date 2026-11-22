//! Cycle solaire : heures de lever et de coucher, et part de « jour » à un instant.
//!
//! Le lieu vient du fuseau horaire du Mac (coordonnées de la ville de référence
//! dans `/usr/share/zoneinfo/zone.tab`) : aucune autorisation de localisation.
//! Calcul du soleil : équations simplifiées de la NOAA (précision ~1 à 2 min).

use std::f64::consts::PI;

/// Lieu de référence du fuseau horaire.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// Ville du fuseau (ex. « Paris »).
    pub city: String,
    pub lat: f64,
    pub lon: f64,
}

/// Instant présent, en heure locale.
#[derive(Debug, Clone, Copy)]
pub struct Now {
    /// Secondes depuis l'époque Unix.
    pub unix: i64,
    /// Minutes écoulées depuis minuit, heure locale.
    pub minute: f64,
    /// Jour de l'année (1 à 366).
    pub yday: u32,
    /// Décalage avec UTC, en minutes.
    pub offset: f64,
}

/// Heures du soleil pour un jour, en minutes depuis minuit (heure locale).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunTimes {
    pub sunrise: f64,
    pub sunset: f64,
}

pub fn now() -> Now {
    unsafe {
        let mut t: libc::time_t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&mut t, &mut tm);
        Now {
            unix: t as i64,
            minute: tm.tm_hour as f64 * 60.0 + tm.tm_min as f64 + tm.tm_sec as f64 / 60.0,
            yday: tm.tm_yday as u32 + 1,
            offset: tm.tm_gmtoff as f64 / 60.0,
        }
    }
}

/// Lieu du fuseau horaire du Mac ; à défaut, une approximation par le décalage UTC.
pub fn place(offset_min: f64) -> Place {
    let tz = std::fs::read_link("/etc/localtime")
        .ok()
        .and_then(|p| p.to_string_lossy().split("zoneinfo/").nth(1).map(str::to_string));
    let table = std::fs::read_to_string("/usr/share/zoneinfo/zone.tab").unwrap_or_default();
    tz.and_then(|tz| place_in(&table, &tz)).unwrap_or(Place {
        city: String::new(),
        lat: 45.0,
        lon: offset_min / 4.0,
    })
}

/// Cherche un fuseau dans le contenu de `zone.tab`.
fn place_in(table: &str, tz: &str) -> Option<Place> {
    let line = table.lines().filter(|l| !l.starts_with('#')).find(|l| l.split('\t').nth(2) == Some(tz))?;
    let (lat, lon) = parse_coords(line.split('\t').nth(1)?)?;
    let city = tz.rsplit('/').next().unwrap_or(tz).replace('_', " ");
    Some(Place { city, lat, lon })
}

/// « +4852+00220 » ou « +404251-0740023 » (degrés, minutes, secondes facultatives).
fn parse_coords(s: &str) -> Option<(f64, f64)> {
    let split = s[1..].find(['+', '-'])? + 1;
    let (a, b) = s.split_at(split);
    Some((dms(a, 2)?, dms(b, 3)?))
}

fn dms(s: &str, deg_len: usize) -> Option<f64> {
    let sign = if s.starts_with('-') { -1.0 } else { 1.0 };
    let digits = &s[1..];
    let part = |from: usize, len: usize| digits.get(from..from + len).map(|p| p.parse::<f64>().ok()).unwrap_or(Some(0.0));
    let d = part(0, deg_len)?;
    let m = part(deg_len, 2)?;
    let sec = part(deg_len + 2, 2)?;
    Some(sign * (d + m / 60.0 + sec / 3600.0))
}

/// Lever et coucher du soleil (minutes locales). `None` : nuit ou jour polaire.
pub fn sun_times(lat: f64, lon: f64, yday: u32, offset_min: f64) -> Option<SunTimes> {
    let g = 2.0 * PI / 365.0 * (yday as f64 - 1.0);
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos() - 0.032077 * g.sin() - 0.014615 * (2.0 * g).cos() - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let phi = lat.to_radians();
    let cos_ha = 90.833_f64.to_radians().cos() / (phi.cos() * decl.cos()) - phi.tan() * decl.tan();
    if !(-1.0..=1.0).contains(&cos_ha) {
        return None;
    }
    let ha = cos_ha.acos().to_degrees();
    let noon = 720.0 - 4.0 * lon - eqtime + offset_min;
    Some(SunTimes { sunrise: noon - 4.0 * ha, sunset: noon + 4.0 * ha })
}

/// Part de « jour » (0 = nuit, 1 = jour) à la minute `m`, passage progressif de
/// `transition` minutes centré sur le lever et le coucher.
pub fn day_factor(m: f64, sun: SunTimes, transition: f64) -> f32 {
    let half = (transition / 2.0).max(1.0);
    let ramp = |x: f64| {
        let t = ((x + half) / (2.0 * half)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    (ramp(m - sun.sunrise) * (1.0 - ramp(m - sun.sunset))) as f32
}

/// Vrai pendant un passage (autour du lever ou du coucher), même tout au début ou à la fin.
pub fn in_transition(m: f64, sun: SunTimes, transition: f64) -> bool {
    let half = (transition / 2.0).max(1.0);
    let near = |t: f64| ((m - t + 720.0).rem_euclid(1440.0) - 720.0).abs() < half;
    near(sun.sunrise) || near(sun.sunset)
}

/// Minute locale (éventuellement ≥ 1440 : le lendemain) du prochain début de passage.
pub fn next_transition(m: f64, sun: SunTimes, transition: f64) -> f64 {
    let half = transition / 2.0;
    [sun.sunrise - half, sun.sunset - half, sun.sunrise - half + 1440.0]
        .into_iter()
        .find(|&s| s > m)
        .unwrap_or(sun.sunrise - half + 1440.0)
}

/// Luminosité visée entre la valeur de nuit et celle de jour.
pub fn blend(day: f32, night: f32, factor: f32) -> f32 {
    night + (day - night) * factor
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZONES: &str = "# commentaire\nFR\t+4852+00220\tEurope/Paris\nUS\t+404251-0740023\tAmerica/New_York\tEastern (most areas)\nAR\t-3436-05827\tAmerica/Argentina/Buenos_Aires\n";

    fn hm(m: f64) -> String {
        format!("{:02}:{:02}", (m / 60.0) as u32, (m % 60.0).round() as u32)
    }

    #[test]
    fn lieu_depuis_zone_tab() {
        let p = place_in(ZONES, "Europe/Paris").unwrap();
        assert_eq!(p.city, "Paris");
        assert!((p.lat - 48.867).abs() < 0.01 && (p.lon - 2.333).abs() < 0.01);
        let ny = place_in(ZONES, "America/New_York").unwrap();
        assert!((ny.lat - 40.714).abs() < 0.01 && (ny.lon + 74.006).abs() < 0.01);
        let ba = place_in(ZONES, "America/Argentina/Buenos_Aires").unwrap();
        assert_eq!(ba.city, "Buenos Aires");
        assert!(ba.lat < -34.0 && ba.lon < -58.0);
        assert!(place_in(ZONES, "Europe/Nulle_Part").is_none());
    }

    #[test]
    fn heures_du_soleil_a_paris() {
        // 21 novembre (jour 325), heure d'hiver : lever 08:08, coucher 17:04 (algorithme NOAA complet).
        let s = sun_times(48.867, 2.333, 325, 60.0).unwrap();
        assert!((s.sunrise - (8.0 * 60.0 + 8.0)).abs() < 3.0, "lever {}", hm(s.sunrise));
        assert!((s.sunset - (17.0 * 60.0 + 4.0)).abs() < 3.0, "coucher {}", hm(s.sunset));
        // 21 juin (jour 172), heure d'été : lever 05:47, coucher 21:57.
        let s = sun_times(48.867, 2.333, 172, 120.0).unwrap();
        assert!((s.sunrise - (5.0 * 60.0 + 47.0)).abs() < 3.0, "lever {}", hm(s.sunrise));
        assert!((s.sunset - (21.0 * 60.0 + 57.0)).abs() < 3.0, "coucher {}", hm(s.sunset));
        // Nuit polaire au nord de la Norvège en décembre.
        assert!(sun_times(78.2, 15.6, 355, 60.0).is_none());
    }

    #[test]
    fn part_de_jour_et_passage() {
        let sun = SunTimes { sunrise: 480.0, sunset: 1020.0 };
        assert_eq!(day_factor(120.0, sun, 40.0), 0.0);
        assert_eq!(day_factor(720.0, sun, 40.0), 1.0);
        assert!((day_factor(480.0, sun, 40.0) - 0.5).abs() < 1e-6);
        assert!((day_factor(1020.0, sun, 40.0) - 0.5).abs() < 1e-6);
        assert_eq!(day_factor(1100.0, sun, 40.0), 0.0);
        assert_eq!(next_transition(720.0, sun, 40.0), 1000.0);
        assert_eq!(next_transition(1100.0, sun, 40.0), 460.0 + 1440.0);
        assert_eq!(blend(80.0, 30.0, 0.5), 55.0);
    }
}
