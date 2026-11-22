//! « Plus sombre » : sous le minimum matériel, l'image est assombrie en abaissant
//! la table de couleurs de l'écran (gamma). Ni fenêtre par-dessus, ni enregistrement
//! de l'écran. Quartz rétablit les tables d'origine quand Dusk quitte ou plante.

use crate::cg::CGDirectDisplayID;
use std::collections::HashMap;
use std::sync::Mutex;

/// Bas de la plage des curseurs : de `DIM_MIN` à 0, l'écran est à son minimum
/// et Dusk assombrit l'image en plus.
pub const DIM_MIN: f32 = -25.0;

/// Part de lumière gardée au bas de la plage (30 % : sombre mais lisible).
const DARKEST: f32 = 0.3;

const SAMPLES: usize = 256;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetDisplayTransferByTable(
        display: CGDirectDisplayID,
        capacity: u32,
        red: *mut f32,
        green: *mut f32,
        blue: *mut f32,
        count: *mut u32,
    ) -> i32;
    fn CGSetDisplayTransferByTable(
        display: CGDirectDisplayID,
        size: u32,
        red: *const f32,
        green: *const f32,
        blue: *const f32,
    ) -> i32;
}

struct Tables {
    r: Vec<f32>,
    g: Vec<f32>,
    b: Vec<f32>,
}

/// Tables d'origine des écrans assombris (calibrage respecté), et facteur appliqué.
static DIMMED: Mutex<Option<HashMap<CGDirectDisplayID, (Tables, f32)>>> = Mutex::new(None);

fn read(id: CGDirectDisplayID) -> Option<Tables> {
    let (mut r, mut g, mut b) = (vec![0.0; SAMPLES], vec![0.0; SAMPLES], vec![0.0; SAMPLES]);
    let mut n = 0u32;
    let err = unsafe {
        CGGetDisplayTransferByTable(id, SAMPLES as u32, r.as_mut_ptr(), g.as_mut_ptr(), b.as_mut_ptr(), &mut n)
    };
    if err != 0 || n == 0 {
        return None;
    }
    let n = n as usize;
    r.truncate(n);
    g.truncate(n);
    b.truncate(n);
    Some(Tables { r, g, b })
}

fn write(id: CGDirectDisplayID, t: &Tables, factor: f32) -> bool {
    let scale = |v: &[f32]| v.iter().map(|x| x * factor).collect::<Vec<f32>>();
    let (r, g, b) = (scale(&t.r), scale(&t.g), scale(&t.b));
    unsafe { CGSetDisplayTransferByTable(id, r.len() as u32, r.as_ptr(), g.as_ptr(), b.as_ptr()) == 0 }
}

/// Part de lumière gardée pour une valeur de curseur (1 dès 0 %).
pub fn factor(level: f32) -> f32 {
    if level >= 0.0 {
        1.0
    } else {
        let t = (level / DIM_MIN).clamp(0.0, 1.0);
        1.0 - (1.0 - DARKEST) * t
    }
}

/// Écran sans canal matériel : tout le curseur passe par l'image (0 % garde 20 %).
pub fn software_factor(level: f32) -> f32 {
    0.2 + 0.8 * (level / 100.0).clamp(0.0, 1.0)
}

/// Applique `level` à l'image de l'écran `id` : la part sous zéro, ou tout le curseur
/// pour un écran sans canal matériel (`software`).
pub fn apply(id: CGDirectDisplayID, level: f32, software: bool) {
    let f = if software { software_factor(level) } else { factor(level) };
    let mut guard = DIMMED.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    if f >= 1.0 {
        if let Some((orig, _)) = map.remove(&id) {
            write(id, &orig, 1.0);
        }
        return;
    }
    if let Some((orig, applied)) = map.get_mut(&id) {
        if (*applied - f).abs() > f32::EPSILON && write(id, orig, f) {
            *applied = f;
        }
        return;
    }
    if let Some(orig) = read(id) {
        if write(id, &orig, f) {
            map.insert(id, (orig, f));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facteur() {
        assert_eq!(factor(40.0), 1.0);
        assert_eq!(factor(0.0), 1.0);
        assert!((factor(DIM_MIN) - DARKEST).abs() < 1e-6);
        assert!(factor(-10.0) < 1.0 && factor(-10.0) > factor(-20.0));
        assert_eq!(software_factor(100.0), 1.0);
        assert!((software_factor(0.0) - 0.2).abs() < 1e-6);
    }
}
