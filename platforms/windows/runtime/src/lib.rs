
#![allow(clippy::missing_safety_doc)]

pub mod advances;
pub mod font;
pub mod game;
pub mod log;
pub mod lse;
pub mod pack;
pub mod patch;
pub mod pe;
pub mod proxy;
pub mod subs;
mod telop;
pub mod vfs;

use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

pub static SELF_MODULE: AtomicUsize = AtomicUsize::new(0);

#[no_mangle]
pub unsafe extern "system" fn DllMain(module: *mut c_void, reason: u32, _reserved: *mut c_void) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        SELF_MODULE.store(module as usize, Ordering::Relaxed);
        install();
    }
    1
}

fn install() {
    log::init();
    if !game::check() {
        log::write("refrain.exe не той сборки, перехваты не ставлю");
        return;
    }
    match pack::open() {
        Ok(n) => log::write(&format!("пак открыт: записей {n}")),
        Err(e) => {
            log::write(&format!("пак не открыт: {e}"));
            return;
        }
    }
    vfs::install();
    lse::install();
    font::install();
    subs::install();
    telop::install();
}
