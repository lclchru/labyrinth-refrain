
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;

static ON: AtomicBool = AtomicBool::new(false);
static FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

pub fn game_dir() -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(core::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let p = PathBuf::from(String::from_utf16_lossy(&buf[..n]));
    p.parent().map(|d| d.to_path_buf())
}

pub fn init() {
    let Some(dir) = game_dir() else { return };
    if !dir.join("refrain-ru.log.on").exists() {
        return;
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(dir.join("refrain-ru.log")) {
        let _ = FILE.set(Mutex::new(f));
        ON.store(true, Ordering::Relaxed);
        write("---- запуск ----");
    }
}

pub fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn write(s: &str) {
    if !enabled() {
        return;
    }
    if let Some(m) = FILE.get() {
        if let Ok(mut f) = m.lock() {
            let _ = writeln!(f, "{s}");
        }
    }
}
