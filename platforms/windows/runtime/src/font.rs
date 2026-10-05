
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::System::Memory::{VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE};

use crate::advances::{ADVANCES, CELLS, REMAP};
use crate::{log, patch};

const BUILD_QUADS: usize = 0x004f_e6c0;
const STOLEN: [u8; 9] = [0x55, 0x8b, 0xec, 0x81, 0xec, 0xf8, 0x00, 0x00, 0x00];

static TRAMP: AtomicUsize = AtomicUsize::new(0);

fn seen() -> &'static Mutex<HashSet<usize>> {
    static S: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashSet::new()))
}

fn advance(code: u32, cell: usize) -> Option<f32> {
    let code = REMAP.iter().find(|(from, _)| *from == code).map_or(code, |(_, to)| *to);
    let i = ADVANCES.binary_search_by_key(&code, |(c, _)| *c).ok()?;
    let a = ADVANCES[i].1[cell];
    (a != 0).then_some(a as f32)
}

#[no_mangle]
unsafe extern "C" fn refrain_fix_font(obj: usize) {
    if obj == 0 {
        return;
    }
    let data = *((obj + 292) as *const usize);
    if data == 0 || !seen().lock().unwrap().insert(data) {
        return;
    }
    let cell_px = *((data + 4) as *const u32);
    let Some(cell) = CELLS.iter().position(|&c| c == cell_px) else {
        log::write(&format!("шрифт с ячейкой {cell_px} не наш, метрика не тронута"));
        return;
    };
    let line = (data + 8) as *mut f32;
    let old_line = *line;
    if old_line > 4.0 {
        *line = (old_line * 0.76).round().max(4.0);
    }
    let count = (*((data + 16) as *const u32) & 0x7fff_ffff) as usize;
    let glyphs = *((data + 20) as *const usize);
    let mut fixed = 0;
    if glyphs != 0 && (1..=100_000).contains(&count) {
        for i in 0..count {
            let g = glyphs + 48 * i;
            if let Some(a) = advance(*(g as *const u32), cell) {
                *((g + 36) as *mut f32) = a;
                fixed += 1;
            }
        }
    }
    log::write(&format!("шрифт {cell_px}: ширин проставлено {fixed}, строка {old_line} -> {}", *line));
}

unsafe fn make_stub() -> Option<usize> {
    let p = VirtualAlloc(core::ptr::null(), 64, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as *mut u8;
    if p.is_null() {
        return None;
    }
    let mut code: Vec<u8> = vec![0x60, 0x9c, 0x51, 0xb8];
    code.extend((refrain_fix_font as *const () as usize as u32).to_le_bytes());
    code.extend([0xff, 0xd0, 0x83, 0xc4, 0x04, 0x9d, 0x61, 0xff, 0x25]);
    code.extend((&TRAMP as *const AtomicUsize as usize as u32).to_le_bytes());
    core::ptr::copy_nonoverlapping(code.as_ptr(), p, code.len());
    Some(p as usize)
}

pub fn install() {
    unsafe {
        let Some(stub) = make_stub() else {
            log::write("память под переходник шрифта не выделена");
            return;
        };
        match patch::hook(BUILD_QUADS, &STOLEN, stub) {
            Ok(t) => {
                TRAMP.store(t, Ordering::Relaxed);
                log::write("построитель текста перехвачен, ширины кириллицы будут проставлены");
            }
            Err(e) => log::write(&format!("построитель текста не перехвачен: {e}")),
        }
    }
}
