//! Écran intégré et écrans Apple via le framework privé `DisplayServices`.

use std::ffi::c_void;
use std::sync::OnceLock;

type CanChange = unsafe extern "C" fn(u32) -> bool;
type GetBrightness = unsafe extern "C" fn(u32, *mut f32) -> i32;
type SetBrightness = unsafe extern "C" fn(u32, f32) -> i32;

struct Api {
    can_change: CanChange,
    get: GetBrightness,
    set: SetBrightness,
}

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let h = libc::dlopen(
            c"/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices".as_ptr(),
            libc::RTLD_LAZY,
        );
        if h.is_null() {
            return None;
        }
        let sym = |n: &std::ffi::CStr| {
            let p = libc::dlsym(h, n.as_ptr());
            (!p.is_null()).then_some(p)
        };
        Some(Api {
            can_change: std::mem::transmute::<*mut c_void, CanChange>(sym(c"DisplayServicesCanChangeBrightness")?),
            get: std::mem::transmute::<*mut c_void, GetBrightness>(sym(c"DisplayServicesGetBrightness")?),
            set: std::mem::transmute::<*mut c_void, SetBrightness>(sym(c"DisplayServicesSetBrightness")?),
        })
    })
    .as_ref()
}

pub fn can_change(id: u32) -> bool {
    api().is_some_and(|a| unsafe { (a.can_change)(id) })
}

/// Luminosité entre 0.0 et 1.0.
pub fn get(id: u32) -> Option<f32> {
    let a = api()?;
    let mut v = 0f32;
    (unsafe { (a.get)(id, &mut v) } == 0).then_some(v)
}

pub fn set(id: u32, value: f32) -> bool {
    api().is_some_and(|a| unsafe { (a.set)(id, value.clamp(0.0, 1.0)) } == 0)
}
