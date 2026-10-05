
use core::ffi::c_void;

use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS,
};
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

pub unsafe fn hook(target: usize, stolen: &[u8], hook: usize) -> Result<usize, String> {
    let n = stolen.len();
    if n < 5 {
        return Err("меньше 5 байт".into());
    }
    let cur = core::slice::from_raw_parts(target as *const u8, n);
    if cur != stolen {
        return Err(format!("байты по {target:#x} не совпали: {cur:02x?}"));
    }
    let tramp = VirtualAlloc(core::ptr::null(), n + 5, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as *mut u8;
    if tramp.is_null() {
        return Err("память под трамплин не выделена".into());
    }
    core::ptr::copy_nonoverlapping(stolen.as_ptr(), tramp, n);
    write_jmp(tramp.add(n), target + n);

    let mut old: PAGE_PROTECTION_FLAGS = 0;
    if VirtualProtect(target as *mut c_void, n, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
        return Err("защита страницы не снята".into());
    }
    let p = target as *mut u8;
    write_jmp(p, hook);
    for i in 5..n {
        *p.add(i) = 0x90;
    }
    let mut back = 0;
    VirtualProtect(target as *mut c_void, n, old, &mut back);
    FlushInstructionCache(GetCurrentProcess(), target as *const c_void, n);
    Ok(tramp as usize)
}

unsafe fn write_jmp(at: *mut u8, to: usize) {
    *at = 0xE9;
    let rel = (to as isize - (at as isize + 5)) as i32;
    core::ptr::write_unaligned(at.add(1) as *mut i32, rel);
}
