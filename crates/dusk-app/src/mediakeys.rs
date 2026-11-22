//! Interception des touches luminosité du clavier (F1/F2, touches soleil).
//!
//! Selon le clavier, elles arrivent comme touches (codes 144/145) ou comme
//! événements « system defined » (type 14, sous-type 8).
//! Un CGEventTap actif les capte et les avale ; il exige l'autorisation
//! Accessibilité (Réglages Système › Confidentialité et sécurité).

use objc2::encode::{Encode, Encoding};
use objc2::rc::Retained;
use objc2::{msg_send, ClassType};
use objc2_app_kit::NSEvent;
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy)]
pub struct KeyPress {
    pub up: bool,
    /// ⇧ : pas fin.
    pub fine: bool,
    /// ⌥ : tous les écrans.
    pub all: bool,
}

const NX_SYSDEFINED: u32 = 14;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const KEYCODE_FIELD: u32 = 9;
const KEYCODE_BRIGHTNESS_UP: i64 = 144;
const KEYCODE_BRIGHTNESS_DOWN: i64 = 145;
const NX_SUBTYPE_AUX_CONTROL_BUTTONS: i16 = 8;
const NX_KEYTYPE_BRIGHTNESS_UP: u32 = 2;
const NX_KEYTYPE_BRIGHTNESS_DOWN: u32 = 3;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;
const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_OPTION: u64 = 0x0008_0000;

type TapCallback = extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, cb: TapCallback, user: *mut c_void) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetFlags(event: *mut c_void) -> u64;
    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(alloc: *const c_void, port: *mut c_void, order: isize) -> *mut c_void;
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static PENDING: Mutex<Vec<KeyPress>> = Mutex::new(Vec::new());

/// `CGEventRef`, encodé comme l'attend `+[NSEvent eventWithCGEvent:]`.
#[repr(transparent)]
struct CGEventRef(*mut c_void);
unsafe impl Encode for CGEventRef {
    const ENCODING: Encoding = Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
}

extern "C" fn callback(_proxy: *mut c_void, ty: u32, event: *mut c_void, _user: *mut c_void) -> *mut c_void {
    if ty == TAP_DISABLED_BY_TIMEOUT || ty == TAP_DISABLED_BY_USER {
        let tap = TAP.load(Ordering::Relaxed);
        if !tap.is_null() {
            unsafe { CGEventTapEnable(tap, true) };
        }
        return event;
    }
    // Claviers récents : touches luminosité = événements clavier (codes 144 / 145).
    if ty == KEY_DOWN || ty == KEY_UP {
        let code = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) };
        if code != KEYCODE_BRIGHTNESS_UP && code != KEYCODE_BRIGHTNESS_DOWN {
            return event;
        }
        if ty == KEY_DOWN {
            push(code == KEYCODE_BRIGHTNESS_UP, unsafe { CGEventGetFlags(event) });
        }
        return std::ptr::null_mut();
    }
    // Claviers plus anciens : événements « system defined ».
    if ty != NX_SYSDEFINED {
        return event;
    }
    let ns: Option<Retained<NSEvent>> = unsafe { msg_send![NSEvent::class(), eventWithCGEvent: CGEventRef(event)] };
    let Some(ns) = ns else { return event };
    if ns.subtype().0 != NX_SUBTYPE_AUX_CONTROL_BUTTONS {
        return event;
    }
    let data1 = ns.data1() as u64;
    let code = ((data1 & 0xFFFF_0000) >> 16) as u32;
    if code != NX_KEYTYPE_BRIGHTNESS_UP && code != NX_KEYTYPE_BRIGHTNESS_DOWN {
        return event;
    }
    let key_down = ((data1 & 0xFF00) >> 8) == 0x0A;
    if key_down {
        push(code == NX_KEYTYPE_BRIGHTNESS_UP, unsafe { CGEventGetFlags(event) });
    }
    // Appui et relâchement sont avalés : macOS n'agit pas en parallèle.
    std::ptr::null_mut()
}

fn push(up: bool, flags: u64) {
    PENDING.lock().unwrap().push(KeyPress {
        up,
        fine: flags & FLAG_SHIFT != 0,
        all: flags & FLAG_OPTION != 0,
    });
}

/// Vérifie l'autorisation Accessibilité ; `prompt` affiche la demande système.
pub fn is_trusted(prompt: bool) -> bool {
    let key = NSString::from_str("AXTrustedCheckOptionPrompt");
    let value = NSNumber::new_bool(prompt);
    let opts = NSDictionary::from_slices(&[&*key], &[&*value]);
    unsafe { AXIsProcessTrustedWithOptions(Retained::as_ptr(&opts).cast()) }
}

/// Installe le tap sur la boucle principale. Renvoie `false` sans autorisation.
pub fn start() -> bool {
    if !TAP.load(Ordering::Relaxed).is_null() {
        return true;
    }
    unsafe {
        // kCGSessionEventTap = 1, kCGHeadInsertEventTap = 0, kCGEventTapOptionDefault = 0
        let mask = (1u64 << NX_SYSDEFINED) | (1 << KEY_DOWN) | (1 << KEY_UP);
        let tap = CGEventTapCreate(1, 0, 0, mask, callback, std::ptr::null_mut());
        if tap.is_null() {
            return false;
        }
        TAP.store(tap, Ordering::Relaxed);
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
    }
    true
}

pub fn drain() -> Vec<KeyPress> {
    std::mem::take(&mut *PENDING.lock().unwrap())
}
