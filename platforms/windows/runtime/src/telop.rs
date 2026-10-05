
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{log, patch, subs};

const SHOW_TELOP: usize = 0x0054_0010;
const STOLEN: [u8; 5] = [0x55, 0x8b, 0xec, 0x53, 0x56];

const IDX_VICTORY: i32 = 15;
const IDX_LOSE: i32 = 16;
const SHOW_MS: u64 = 3200;

static TRAMP: AtomicUsize = AtomicUsize::new(0);

#[no_mangle]
unsafe extern "C" fn refrain_on_telop(args: *const u8) {
    if args.is_null() {
        return;
    }
    let p = *(args as *const *const i32);
    if p.is_null() {
        return;
    }
    let text = match *p {
        IDX_VICTORY => "ПОБЕДА",
        IDX_LOSE => "ПОРАЖЕНИЕ",
        _ => return,
    };
    log::write(&format!("телоп: {text}"));
    subs::show_telop(text, SHOW_MS);
}

unsafe fn make_stub() -> Option<usize> {
    use windows_sys::Win32::System::Memory::{VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE};
    let p = VirtualAlloc(core::ptr::null(), 64, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as *mut u8;
    if p.is_null() {
        return None;
    }
    let mut c: Vec<u8> = vec![0x60, 0x9c, 0x8d, 0x44, 0x24, 0x28, 0x50, 0xb8];
    c.extend((refrain_on_telop as *const () as usize as u32).to_le_bytes());
    c.extend([0xff, 0xd0, 0x83, 0xc4, 0x04, 0x9d, 0x61, 0xff, 0x25]);
    c.extend((&TRAMP as *const AtomicUsize as usize as u32).to_le_bytes());
    core::ptr::copy_nonoverlapping(c.as_ptr(), p, c.len());
    Some(p as usize)
}

pub fn install() {
    unsafe {
        let Some(stub) = make_stub() else { return };
        match patch::hook(SHOW_TELOP, &STOLEN, stub) {
            Ok(t) => {
                TRAMP.store(t, Ordering::Relaxed);
                log::write("телопы боя перехвачены");
            }
            Err(e) => log::write(&format!("телопы боя не перехвачены: {e}")),
        }
    }
}
