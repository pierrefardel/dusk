//! Accès bas niveau à l'IORegistry et aux symboles privés `IOAVService*`.

use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

pub type IoObject = u32;
pub type IOAVServiceRef = *mut c_void;

const K_IO_MAIN_PORT_DEFAULT: u32 = 0;
const K_IO_REGISTRY_ITERATE_RECURSIVELY: u32 = 1;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IORegistryGetRootEntry(main_port: u32) -> IoObject;
    fn IORegistryEntryCreateIterator(
        entry: IoObject,
        plane: *const c_char,
        options: u32,
        iterator: *mut IoObject,
    ) -> i32;
    fn IOIteratorNext(iterator: IoObject) -> IoObject;
    pub fn IOObjectRelease(object: IoObject) -> i32;
    fn IORegistryEntryGetName(entry: IoObject, name: *mut c_char) -> i32;
    fn IORegistryEntryCreateCFProperty(
        entry: IoObject,
        key: CFStringRef,
        allocator: *const c_void,
        options: u32,
    ) -> CFTypeRef;
}

/// Parcourt récursivement le plan IOService et appelle `f(entry, name)` pour chaque nœud.
/// L'entrée est libérée après l'appel : `f` doit la retenir s'il en a besoin.
pub fn walk_service_plane(mut f: impl FnMut(IoObject, &str)) {
    unsafe {
        let root = IORegistryGetRootEntry(K_IO_MAIN_PORT_DEFAULT);
        let mut iter: IoObject = 0;
        let plane = c"IOService";
        if IORegistryEntryCreateIterator(root, plane.as_ptr(), K_IO_REGISTRY_ITERATE_RECURSIVELY, &mut iter) != 0 {
            IOObjectRelease(root);
            return;
        }
        loop {
            let entry = IOIteratorNext(iter);
            if entry == 0 {
                break;
            }
            let mut buf = [0 as c_char; 128];
            if IORegistryEntryGetName(entry, buf.as_mut_ptr()) == 0 {
                let name = CStr::from_ptr(buf.as_ptr()).to_string_lossy();
                f(entry, &name);
            }
            IOObjectRelease(entry);
        }
        IOObjectRelease(iter);
        IOObjectRelease(root);
    }
}

pub fn property(entry: IoObject, key: &str) -> Option<CFType> {
    let key = CFString::new(key);
    let ptr = unsafe {
        IORegistryEntryCreateCFProperty(entry, key.as_concrete_TypeRef(), std::ptr::null(), 0)
    };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { CFType::wrap_under_create_rule(ptr) })
    }
}

pub fn dict_get(dict: &CFDictionary<CFString, CFType>, key: &str) -> Option<CFType> {
    dict.find(CFString::new(key)).map(|v| v.clone())
}

pub fn as_dict(v: &CFType) -> Option<CFDictionary<CFString, CFType>> {
    v.downcast::<CFDictionary>().map(|d| unsafe {
        CFDictionary::wrap_under_get_rule(d.as_concrete_TypeRef())
    })
}

pub fn as_string(v: &CFType) -> Option<String> {
    v.downcast::<CFString>().map(|s| s.to_string())
}

pub fn as_i64(v: &CFType) -> Option<i64> {
    v.downcast::<CFNumber>().and_then(|n| n.to_i64())
}

// --- Symboles privés IOAVService, résolus dynamiquement ---------------------

type CreateWithService = unsafe extern "C" fn(*const c_void, IoObject) -> IOAVServiceRef;
type I2CFn = unsafe extern "C" fn(IOAVServiceRef, u32, u32, *mut u8, u32) -> i32;

pub struct AvApi {
    pub create_with_service: CreateWithService,
    pub write_i2c: I2CFn,
    pub read_i2c: I2CFn,
}

pub fn av_api() -> Option<&'static AvApi> {
    static API: OnceLock<Option<AvApi>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let handle = libc::dlopen(
            c"/System/Library/Frameworks/IOKit.framework/IOKit".as_ptr(),
            libc::RTLD_LAZY,
        );
        if handle.is_null() {
            return None;
        }
        let sym = |name: &CStr| {
            let p = libc::dlsym(handle, name.as_ptr());
            (!p.is_null()).then_some(p)
        };
        Some(AvApi {
            create_with_service: std::mem::transmute::<*mut c_void, CreateWithService>(
                sym(c"IOAVServiceCreateWithService")?,
            ),
            write_i2c: std::mem::transmute::<*mut c_void, I2CFn>(sym(c"IOAVServiceWriteI2C")?),
            read_i2c: std::mem::transmute::<*mut c_void, I2CFn>(sym(c"IOAVServiceReadI2C")?),
        })
    })
    .as_ref()
}

extern "C" {
    pub fn CFRelease(cf: *const c_void);
}
