
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{game, log, pack, patch};

const STOLEN: [u8; 5] = [0x55, 0x8b, 0xec, 0x6a, 0xff];

static TRAMP: AtomicUsize = AtomicUsize::new(0);

type LoadFn = unsafe extern "C" fn(u32, u32, u32, u32, u32, u32, *mut *mut u8, *mut u32) -> u8;
type AllocFn = unsafe extern "C" fn(u32, u32) -> *mut u8;
type DeleteFn = unsafe extern "C" fn(*mut u8);

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn load(a0: u32, a1: u32, a2: u32, a3: u32, size: u32, cap: u32, out: *mut *mut u8, out_size: *mut u32) -> u8 {
    let inline: [u8; 16] = core::mem::transmute([a0, a1, a2, a3]);
    let path: &[u8] = if cap >= 16 {
        core::slice::from_raw_parts(a0 as *const u8, size as usize)
    } else {
        &inline[..(size as usize).min(15)]
    };
    let key = String::from_utf8_lossy(path).replace('\\', "/").to_ascii_lowercase();

    if pack::contains(&key) {
        if let Some(data) = pack::read(&key) {
            if let Some(n) = data.len().checked_sub(1) {
                let alloc: AllocFn = core::mem::transmute(game::ALIGNED_ALLOC);
                let buf = alloc(n as u32 + 1, 1);
                if !buf.is_null() {
                    let seed = data[n];
                    for (i, &b) in data[..n].iter().enumerate() {
                        *buf.add(i) = b ^ seed.wrapping_add(i as u8);
                    }
                    *buf.add(n) = 0;
                    *out = buf;
                    *out_size = n as u32;
                    if cap >= 16 {
                        let del: DeleteFn = core::mem::transmute(game::OPERATOR_DELETE);
                        del(a0 as *mut u8);
                    }
                    log::write(&format!("текст из пака {key}"));
                    return 1;
                }
            }
        }
        log::write(&format!("запись пака не прочитана: {key}"));
    }
    let orig: LoadFn = core::mem::transmute(TRAMP.load(Ordering::Relaxed));
    orig(a0, a1, a2, a3, size, cap, out, out_size)
}

pub fn install() {
    match unsafe { patch::hook(game::LOAD_RESOURCE, &STOLEN, load as *const () as usize) } {
        Ok(t) => {
            TRAMP.store(t, Ordering::Relaxed);
            log::write("загрузчик .lse перехвачен");
        }
        Err(e) => log::write(&format!("загрузчик .lse не перехвачен: {e}")),
    }
}
