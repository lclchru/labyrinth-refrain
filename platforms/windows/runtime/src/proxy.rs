
use core::ffi::c_void;
use std::sync::OnceLock;

use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

fn system_dll() -> usize {
    static H: OnceLock<usize> = OnceLock::new();
    *H.get_or_init(|| unsafe {
        let mut buf = [0u16; 512];
        let n = GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) as usize;
        if n == 0 || n > buf.len() - 16 {
            return 0;
        }
        let mut path: Vec<u16> = buf[..n].to_vec();
        path.extend("\\dinput8.dll".encode_utf16());
        path.push(0);
        LoadLibraryW(path.as_ptr()) as usize
    })
}

fn proc(name: &'static [u8]) -> usize {
    let h = system_dll();
    if h == 0 {
        return 0;
    }
    unsafe { GetProcAddress(h as _, name.as_ptr()).map_or(0, |f| f as usize) }
}

macro_rules! forward {
    ($name:ident, $export:expr, fn($($an:ident : $at:ty),*) -> $ret:ty, $fallback:expr) => {
        #[no_mangle]
        pub unsafe extern "system" fn $name($($an: $at),*) -> $ret {
            static SLOT: OnceLock<usize> = OnceLock::new();
            let p = *SLOT.get_or_init(|| proc($export));
            if p == 0 {
                return $fallback;
            }
            let f: unsafe extern "system" fn($($at),*) -> $ret = core::mem::transmute(p);
            f($($an),*)
        }
    };
}

const E_FAIL: i32 = 0x8000_4005u32 as i32;

forward!(
    DirectInput8Create,
    b"DirectInput8Create\0",
    fn(inst: *mut c_void, version: u32, riid: *const c_void, out: *mut *mut c_void, outer: *mut c_void) -> i32,
    E_FAIL
);
forward!(DllCanUnloadNow, b"DllCanUnloadNow\0", fn() -> i32, E_FAIL);
forward!(
    DllGetClassObject,
    b"DllGetClassObject\0",
    fn(clsid: *const c_void, iid: *const c_void, out: *mut *mut c_void) -> i32,
    E_FAIL
);
forward!(DllRegisterServer, b"DllRegisterServer\0", fn() -> i32, E_FAIL);
forward!(DllUnregisterServer, b"DllUnregisterServer\0", fn() -> i32, E_FAIL);
forward!(GetdfDIJoystick, b"GetdfDIJoystick\0", fn() -> *mut c_void, core::ptr::null_mut());
