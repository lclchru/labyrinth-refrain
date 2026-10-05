
use core::ffi::c_void;

use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_PROTECTION_FLAGS, PAGE_READWRITE};

pub unsafe fn replace_import(base: usize, func: &str, new: usize) -> (usize, Option<usize>) {
    let nt = base + *((base + 0x3c) as *const i32) as usize;
    let dir_rva = *((nt + 0x80) as *const u32) as usize;
    if dir_rva == 0 {
        return (0, None);
    }
    let mut count = 0;
    let mut prev = None;
    let mut desc = base + dir_rva;
    loop {
        let original_first = *(desc as *const u32) as usize;
        let name_rva = *((desc + 12) as *const u32) as usize;
        let first = *((desc + 16) as *const u32) as usize;
        if name_rva == 0 {
            break;
        }
        let lookup = if original_first != 0 { original_first } else { first };
        let mut i = 0usize;
        loop {
            let entry = *((base + lookup + i * 4) as *const u32);
            if entry == 0 {
                break;
            }
            if entry & 0x8000_0000 == 0 {
                let hint_name = base + entry as usize + 2;
                if cstr_eq(hint_name as *const u8, func) {
                    let slot = (base + first + i * 4) as *mut usize;
                    let mut old: PAGE_PROTECTION_FLAGS = 0;
                    if VirtualProtect(slot as *mut c_void, 4, PAGE_READWRITE, &mut old) != 0 {
                        if prev.is_none() {
                            prev = Some(*slot);
                        }
                        *slot = new;
                        let mut back = 0;
                        VirtualProtect(slot as *mut c_void, 4, old, &mut back);
                        count += 1;
                    }
                }
            }
            i += 1;
        }
        desc += 20;
    }
    (count, prev)
}

unsafe fn cstr_eq(p: *const u8, s: &str) -> bool {
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if *p.add(i) != c {
            return false;
        }
    }
    *p.add(b.len()) == 0
}
