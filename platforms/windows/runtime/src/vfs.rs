
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use core::ffi::c_void;
use windows_sys::Win32::Foundation::{
    SetLastError, ERROR_INVALID_PARAMETER, ERROR_NEGATIVE_SEEK, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    GetTempPathW, WriteFile, FILE_ATTRIBUTE_TEMPORARY, FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, INVALID_SET_FILE_POINTER, SetFilePointerEx, CREATE_ALWAYS,
};

use crate::{log, pack, pe};
use windows_sys::core::BOOL;

const GENERIC_WRITE: u32 = 0x4000_0000;
const OPEN_EXISTING: u32 = 3;
const FILE_BEGIN: u32 = 0;
const FILE_CURRENT: u32 = 1;
const FILE_END: u32 = 2;

const FAKE_BASE: usize = 0x5A00_0000;
const FAKE_LIMIT: usize = 0x5B00_0000;
static NEXT: AtomicUsize = AtomicUsize::new(FAKE_BASE);

struct Open {
    data: Arc<Vec<u8>>,
    pos: u64,
}

fn table() -> &'static Mutex<HashMap<usize, Open>> {
    static T: OnceLock<Mutex<HashMap<usize, Open>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

fn is_fake(h: HANDLE) -> bool {
    let v = h as usize;
    (FAKE_BASE..FAKE_LIMIT).contains(&v) && v % 4 == 0
}

type CreateFileWFn = unsafe extern "system" fn(*const u16, u32, u32, *const c_void, u32, u32, HANDLE) -> HANDLE;
type GetFileSizeExFn = unsafe extern "system" fn(HANDLE, *mut i64) -> BOOL;
type ReadFileFn = unsafe extern "system" fn(HANDLE, *mut u8, u32, *mut u32, *mut c_void) -> BOOL;
type SetFilePointerFn = unsafe extern "system" fn(HANDLE, i32, *mut i32, u32) -> u32;
type HandleFn = unsafe extern "system" fn(HANDLE) -> BOOL;

static ORIG_CREATE: AtomicUsize = AtomicUsize::new(0);
static ORIG_CRT_CREATE: AtomicUsize = AtomicUsize::new(0);
static ORIG_SIZE: AtomicUsize = AtomicUsize::new(0);
static ORIG_READ: AtomicUsize = AtomicUsize::new(0);
static ORIG_SEEK: AtomicUsize = AtomicUsize::new(0);
static ORIG_EOF: AtomicUsize = AtomicUsize::new(0);
static ORIG_CLOSE: AtomicUsize = AtomicUsize::new(0);

unsafe fn wide(p: *const u16) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let mut n = 0usize;
    while n < 32_768 && *p.add(n) != 0 {
        n += 1;
    }
    Some(String::from_utf16_lossy(core::slice::from_raw_parts(p, n)))
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn loose_override(key: &str) -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    let dir = DIR.get_or_init(|| log::game_dir().map(|d| d.join("refrain-ru")).filter(|d| d.is_dir()));
    let p = dir.as_ref()?.join(key.replace('/', "\\"));
    p.is_file().then_some(p)
}

unsafe extern "system" fn create_file_w(
    name: *const u16,
    access: u32,
    share: u32,
    sa: *const c_void,
    disposition: u32,
    flags: u32,
    template: HANDLE,
) -> HANDLE {
    let orig: CreateFileWFn = core::mem::transmute(ORIG_CREATE.load(Ordering::Relaxed));
    if access & GENERIC_WRITE == 0 && disposition == OPEN_EXISTING {
        if let Some(key) = wide(name).and_then(|s| pack::key_from_path(&s)) {
            if let Some(p) = loose_override(&key) {
                log::write(&format!("из папки {key}"));
                let w = to_wide(&p.to_string_lossy());
                return orig(w.as_ptr(), access, share, sa, disposition, flags, template);
            }
            if pack::contains(&key) {
                if let Some(data) = pack::read(&key) {
                    let h = NEXT.fetch_add(4, Ordering::Relaxed);
                    if h < FAKE_LIMIT {
                        table().lock().unwrap().insert(h, Open { data, pos: 0 });
                        log::write(&format!("из пака {key}"));
                        return h as HANDLE;
                    }
                }
                log::write(&format!("запись пака не прочитана: {key}"));
            }
        }
    }
    orig(name, access, share, sa, disposition, flags, template)
}

unsafe extern "system" fn get_file_size_ex(h: HANDLE, out: *mut i64) -> BOOL {
    if is_fake(h) {
        if let Some(f) = table().lock().unwrap().get(&(h as usize)) {
            if !out.is_null() {
                *out = f.data.len() as i64;
            }
            return 1;
        }
    }
    let orig: GetFileSizeExFn = core::mem::transmute(ORIG_SIZE.load(Ordering::Relaxed));
    orig(h, out)
}

unsafe extern "system" fn read_file(h: HANDLE, buf: *mut u8, n: u32, got: *mut u32, ov: *mut c_void) -> BOOL {
    if is_fake(h) && ov.is_null() {
        let mut t = table().lock().unwrap();
        if let Some(f) = t.get_mut(&(h as usize)) {
            let len = f.data.len() as u64;
            let start = f.pos.min(len) as usize;
            let k = (n as usize).min(len as usize - start);
            core::ptr::copy_nonoverlapping(f.data.as_ptr().add(start), buf, k);
            f.pos = (start + k) as u64;
            if !got.is_null() {
                *got = k as u32;
            }
            return 1;
        }
    }
    let orig: ReadFileFn = core::mem::transmute(ORIG_READ.load(Ordering::Relaxed));
    orig(h, buf, n, got, ov)
}

unsafe extern "system" fn set_file_pointer(h: HANDLE, dist: i32, high: *mut i32, method: u32) -> u32 {
    if is_fake(h) {
        let mut t = table().lock().unwrap();
        if let Some(f) = t.get_mut(&(h as usize)) {
            let delta = if high.is_null() { dist as i64 } else { ((*high as i64) << 32) | (dist as u32 as i64) };
            let base = match method {
                FILE_BEGIN => 0i64,
                FILE_CURRENT => f.pos as i64,
                FILE_END => f.data.len() as i64,
                _ => {
                    SetLastError(ERROR_INVALID_PARAMETER);
                    return INVALID_SET_FILE_POINTER;
                }
            };
            let new = base + delta;
            if new < 0 {
                SetLastError(ERROR_NEGATIVE_SEEK);
                return INVALID_SET_FILE_POINTER;
            }
            f.pos = new as u64;
            if !high.is_null() {
                *high = (new >> 32) as i32;
            }
            SetLastError(0);
            return new as u32;
        }
    }
    let orig: SetFilePointerFn = core::mem::transmute(ORIG_SEEK.load(Ordering::Relaxed));
    orig(h, dist, high, method)
}

unsafe extern "system" fn set_end_of_file(h: HANDLE) -> BOOL {
    if is_fake(h) {
        return 1;
    }
    let orig: HandleFn = core::mem::transmute(ORIG_EOF.load(Ordering::Relaxed));
    orig(h)
}

unsafe extern "system" fn close_handle(h: HANDLE) -> BOOL {
    if is_fake(h) && table().lock().unwrap().remove(&(h as usize)).is_some() {
        return 1;
    }
    let orig: HandleFn = core::mem::transmute(ORIG_CLOSE.load(Ordering::Relaxed));
    orig(h)
}

unsafe extern "system" fn crt_create_file_w(
    name: *const u16,
    access: u32,
    share: u32,
    sa: *const c_void,
    disposition: u32,
    flags: u32,
    template: HANDLE,
) -> HANDLE {
    let orig: CreateFileWFn = core::mem::transmute(ORIG_CRT_CREATE.load(Ordering::Relaxed));
    if access & GENERIC_WRITE == 0 && disposition == OPEN_EXISTING {
        if let Some(key) = wide(name).and_then(|s| pack::key_from_path(&s)) {
            if let Some(p) = loose_override(&key) {
                let w = to_wide(&p.to_string_lossy());
                return orig(w.as_ptr(), access, share, sa, disposition, flags, template);
            }
            if let Some(data) = pack::read(&key) {
                log::write(&format!("мимо загрузчика, временный файл: {key}"));
                if let Some(h) = temp_file(orig, &data) {
                    return h;
                }
            }
        }
    }
    orig(name, access, share, sa, disposition, flags, template)
}

unsafe fn temp_file(create: CreateFileWFn, data: &[u8]) -> Option<HANDLE> {
    let mut dir = [0u16; 512];
    let n = GetTempPathW(dir.len() as u32, dir.as_mut_ptr()) as usize;
    if n == 0 || n > dir.len() - 40 {
        return None;
    }
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let name = format!(
        "{}refrain-ru-{}-{}.tmp",
        String::from_utf16_lossy(&dir[..n]),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let w = to_wide(&name);
    let h = create(
        w.as_ptr(),
        0x8000_0000 | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        core::ptr::null(),
        CREATE_ALWAYS,
        FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE,
        core::ptr::null_mut(),
    );
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut written = 0u32;
    if WriteFile(h, data.as_ptr(), data.len() as u32, &mut written, core::ptr::null_mut()) == 0
        || written as usize != data.len()
    {
        windows_sys::Win32::Foundation::CloseHandle(h);
        return None;
    }
    SetFilePointerEx(h, 0, core::ptr::null_mut(), FILE_BEGIN);
    Some(h)
}

pub fn install() {
    let exe = crate::game::exe_base();
    let mut report = Vec::new();
    unsafe {
        for (name, new, slot) in [
            ("CreateFileW", create_file_w as *const () as usize, &ORIG_CREATE),
            ("GetFileSizeEx", get_file_size_ex as *const () as usize, &ORIG_SIZE),
            ("ReadFile", read_file as *const () as usize, &ORIG_READ),
            ("SetFilePointer", set_file_pointer as *const () as usize, &ORIG_SEEK),
            ("SetEndOfFile", set_end_of_file as *const () as usize, &ORIG_EOF),
            ("CloseHandle", close_handle as *const () as usize, &ORIG_CLOSE),
        ] {
            let (n, prev) = pe::replace_import(exe, name, new);
            if let Some(p) = prev {
                slot.store(p, Ordering::Relaxed);
            }
            report.push(format!("{name} {n}"));
        }
        let crt = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(to_wide("MSVCR120.dll").as_ptr()) as usize;
        if crt != 0 {
            let (n, prev) = pe::replace_import(crt, "CreateFileW", crt_create_file_w as *const () as usize);
            if let Some(p) = prev {
                ORIG_CRT_CREATE.store(p, Ordering::Relaxed);
            }
            report.push(format!("CRT CreateFileW {n}"));
        }
    }
    log::write(&format!("слоты импорта: {}", report.join(", ")));
}
