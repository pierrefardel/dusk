//! Capteur de lumière ambiante (MacBook, écrans Apple) via `IOHIDEventSystemClient`.
//! Sans capteur (Mac mini, Mac Studio), `lux()` renvoie `None` et rien ne s'affiche.
//! Test : `DUSK_LUX=<valeur>` simule un capteur qui mesure cette valeur.

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use std::ffi::c_void;
use std::sync::OnceLock;

/// `kIOHIDEventTypeAmbientLightSensor`.
const EVENT_ALS: i64 = 12;
/// Champ « niveau » de l'événement (`IOHIDEventFieldBase(type)`).
const FIELD_LEVEL: i32 = (EVENT_ALS as i32) << 16;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDEventSystemClientCreate(allocator: *const c_void) -> *mut c_void;
    fn IOHIDEventSystemClientSetMatching(client: *mut c_void, matching: *const c_void) -> i32;
    fn IOHIDEventSystemClientCopyServices(client: *mut c_void) -> *const c_void;
    fn IOHIDServiceClientCopyEvent(service: *const c_void, kind: i64, options: i32, timestamp: i64) -> *const c_void;
    fn IOHIDEventGetFloatValue(event: *const c_void, field: i32) -> f64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
}

/// Service du capteur, cherché une fois (le client reste ouvert pour l'app).
struct Sensor(*const c_void);
unsafe impl Send for Sensor {}
unsafe impl Sync for Sensor {}

fn sensor() -> Option<&'static Sensor> {
    static SENSOR: OnceLock<Option<Sensor>> = OnceLock::new();
    SENSOR
        .get_or_init(|| unsafe {
            let client = IOHIDEventSystemClientCreate(std::ptr::null());
            if client.is_null() {
                return None;
            }
            let matching = CFDictionary::from_CFType_pairs(&[
                (CFString::new("PrimaryUsagePage").as_CFType(), CFNumber::from(0xff00_i32).as_CFType()),
                (CFString::new("PrimaryUsage").as_CFType(), CFNumber::from(4_i32).as_CFType()),
            ]);
            IOHIDEventSystemClientSetMatching(client, matching.as_concrete_TypeRef() as *const c_void);
            let services = IOHIDEventSystemClientCopyServices(client);
            if services.is_null() {
                return None;
            }
            // Premier service qui renvoie vraiment une mesure.
            (0..CFArrayGetCount(services))
                .map(|i| CFArrayGetValueAtIndex(services, i))
                .find(|&s| read(s).is_some())
                .map(Sensor)
        })
        .as_ref()
}

unsafe fn read(service: *const c_void) -> Option<f64> {
    let event = IOHIDServiceClientCopyEvent(service, EVENT_ALS, 0, 0);
    if event.is_null() {
        return None;
    }
    let v = IOHIDEventGetFloatValue(event, FIELD_LEVEL);
    CFRelease(event as CFTypeRef);
    (v.is_finite() && v >= 0.0).then_some(v)
}

/// Lumière ambiante en lux, ou `None` sans capteur.
pub fn lux() -> Option<f64> {
    if let Some(v) = std::env::var("DUSK_LUX").ok().and_then(|v| v.parse::<f64>().ok()) {
        return Some(v.max(0.0));
    }
    sensor().and_then(|s| unsafe { read(s.0) })
}

/// Pièce sombre sous ce niveau, pièce claire au-dessus de `BRIGHT` (lux).
const DARK: f64 = 10.0;
const BRIGHT: f64 = 400.0;

/// Position entre pièce sombre (0) et pièce claire (1), sur une échelle logarithmique
/// (l'œil perçoit les rapports de lumière, pas les écarts).
pub fn factor(lux: f64) -> f32 {
    let l = |x: f64| (x + 1.0).log10();
    ((l(lux) - l(DARK)) / (l(BRIGHT) - l(DARK))).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echelle() {
        assert_eq!(factor(0.0), 0.0);
        assert_eq!(factor(DARK), 0.0);
        assert_eq!(factor(BRIGHT), 1.0);
        assert_eq!(factor(5000.0), 1.0);
        let mid = factor(60.0);
        assert!(mid > 0.3 && mid < 0.7, "{mid}");
    }
}
