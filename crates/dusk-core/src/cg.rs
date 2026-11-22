//! Bindings minimaux CoreGraphics pour l'énumération des écrans.

pub type CGDirectDisplayID = u32;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetOnlineDisplayList(max: u32, displays: *mut CGDirectDisplayID, count: *mut u32) -> i32;
    pub fn CGDisplayIsBuiltin(display: CGDirectDisplayID) -> u32;
    pub fn CGDisplayVendorNumber(display: CGDirectDisplayID) -> u32;
    pub fn CGDisplayModelNumber(display: CGDirectDisplayID) -> u32;
    pub fn CGDisplaySerialNumber(display: CGDirectDisplayID) -> u32;
    pub fn CGDisplayIsInMirrorSet(display: CGDirectDisplayID) -> u32;
    pub fn CGDisplayMirrorsDisplay(display: CGDirectDisplayID) -> CGDirectDisplayID;
    fn CGEventCreate(source: *const std::ffi::c_void) -> *mut std::ffi::c_void;
    fn CGEventGetLocation(event: *mut std::ffi::c_void) -> CGPoint;
    fn CGGetDisplaysWithPoint(point: CGPoint, max: u32, displays: *mut CGDirectDisplayID, count: *mut u32) -> i32;
    fn CGBeginDisplayConfiguration(config: *mut *mut std::ffi::c_void) -> i32;
    fn CGCompleteDisplayConfiguration(config: *mut std::ffi::c_void, option: u32) -> i32;
    fn CGCancelDisplayConfiguration(config: *mut std::ffi::c_void) -> i32;
    /// Privé : active ou déconnecte logiciellement un écran.
    fn CGSConfigureDisplayEnabled(config: *mut std::ffi::c_void, display: CGDirectDisplayID, enabled: bool) -> i32;
}

/// Changement valable tant que l'app tourne : si Dusk quitte ou plante, l'écran revient.
const CONFIGURE_FOR_APP_ONLY: u32 = 0;

/// Déconnecte (`false`) ou reconnecte un écran côté macOS. L'écran, privé
/// d'image, se met en veille de lui-même ; le Mac continue de tourner.
pub fn set_display_enabled(id: CGDirectDisplayID, enabled: bool) -> bool {
    unsafe {
        let mut config = std::ptr::null_mut();
        if CGBeginDisplayConfiguration(&mut config) != 0 {
            return false;
        }
        if CGSConfigureDisplayEnabled(config, id, enabled) != 0 {
            CGCancelDisplayConfiguration(config);
            return false;
        }
        CGCompleteDisplayConfiguration(config, CONFIGURE_FOR_APP_ONLY) == 0
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

/// Écran sous le curseur de la souris.
pub fn display_under_cursor() -> Option<CGDirectDisplayID> {
    unsafe {
        let ev = CGEventCreate(std::ptr::null());
        if ev.is_null() {
            return None;
        }
        let p = CGEventGetLocation(ev);
        crate::iokit::CFRelease(ev);
        let (mut id, mut count) = (0u32, 0u32);
        (CGGetDisplaysWithPoint(p, 1, &mut id, &mut count) == 0 && count > 0).then_some(id)
    }
}

pub fn online_displays() -> Vec<CGDirectDisplayID> {
    let mut ids = [0u32; 32];
    let mut count = 0u32;
    let err = unsafe { CGGetOnlineDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    if err != 0 {
        return Vec::new();
    }
    ids[..count as usize]
        .iter()
        .copied()
        // Un écran en recopie vidéo pointe vers son maître : on ne garde que les maîtres.
        .filter(|&id| unsafe { CGDisplayIsInMirrorSet(id) == 0 || CGDisplayMirrorsDisplay(id) == 0 })
        .collect()
}
