
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

pub const IMAGE_BASE: usize = 0x40_0000;
const TIMESTAMP: u32 = 0x5bc8_41c5;
const SIZE_OF_IMAGE: u32 = 0x009a_f000;

pub const LOAD_RESOURCE: usize = 0x0064_2fe0;
pub const ALIGNED_ALLOC: usize = 0x0064_9fc0;
pub const OPERATOR_DELETE: usize = 0x0081_2576;

pub fn exe_base() -> usize {
    unsafe { GetModuleHandleW(core::ptr::null()) as usize }
}

pub fn check() -> bool {
    let base = exe_base();
    if base != IMAGE_BASE {
        return false;
    }
    unsafe {
        let dos = base as *const u8;
        if *(dos as *const u16) != 0x5a4d {
            return false;
        }
        let nt = base + *(dos.add(0x3c) as *const i32) as usize;
        let stamp = *((nt + 8) as *const u32);
        let size = *((nt + 0x50) as *const u32);
        stamp == TIMESTAMP && size == SIZE_OF_IMAGE
    }
}
