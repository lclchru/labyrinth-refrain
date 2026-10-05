
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, OnceLock};

use windows_sys::Win32::Foundation::{GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileSizeEx, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, OPEN_EXISTING,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows_sys::Win32::System::IO::OVERLAPPED;

const MAGIC: &[u8; 8] = b"RFRUPAK1";

struct Entry {
    method: u8,
    offset: u32,
    comp: u32,
    raw: u32,
}

struct Pack {
    file: usize,
    entries: HashMap<String, Entry>,
}

static PACK: OnceLock<Pack> = OnceLock::new();

fn self_path() -> Option<Vec<u16>> {
    let module = crate::SELF_MODULE.load(std::sync::atomic::Ordering::Relaxed);
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(module as _, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    buf.truncate(n);
    buf.push(0);
    Some(buf)
}

fn read_at(file: HANDLE, offset: u64, out: &mut [u8]) -> bool {
    let mut done = 0usize;
    while done < out.len() {
        let pos = offset + done as u64;
        let mut ov: OVERLAPPED = unsafe { core::mem::zeroed() };
        ov.Anonymous.Anonymous.Offset = pos as u32;
        ov.Anonymous.Anonymous.OffsetHigh = (pos >> 32) as u32;
        let want = (out.len() - done).min(64 << 20) as u32;
        let mut got = 0u32;
        let ok = unsafe { ReadFile(file, out[done..].as_mut_ptr(), want, &mut got, &mut ov) };
        if ok == 0 || got == 0 {
            return false;
        }
        done += got as usize;
    }
    true
}

pub fn open() -> Result<usize, String> {
    let path = self_path().ok_or("путь к своей DLL не получен")?;
    let file = unsafe {
        CreateFileW(path.as_ptr(), GENERIC_READ, FILE_SHARE_READ, core::ptr::null(), OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, core::ptr::null_mut())
    };
    if file == INVALID_HANDLE_VALUE {
        return Err("своя DLL не открылась на чтение".into());
    }
    let mut size = 0i64;
    if unsafe { GetFileSizeEx(file, &mut size) } == 0 || size < 16 {
        return Err("размер своей DLL не получен".into());
    }
    let mut tail = [0u8; 16];
    if !read_at(file, size as u64 - 16, &mut tail) {
        return Err("хвост не прочитан".into());
    }
    if &tail[..8] != MAGIC {
        return Err("в DLL нет пака".into());
    }
    let index_off = u32::from_le_bytes(tail[8..12].try_into().unwrap()) as u64;
    let index_len = u32::from_le_bytes(tail[12..16].try_into().unwrap()) as usize;
    let mut index = vec![0u8; index_len];
    if !read_at(file, index_off, &mut index) {
        return Err("оглавление не прочитано".into());
    }
    let entries = parse_index(&index).ok_or("оглавление битое")?;
    let n = entries.len();
    let _ = PACK.set(Pack { file: file as usize, entries });
    Ok(n)
}

fn parse_index(b: &[u8]) -> Option<HashMap<String, Entry>> {
    let mut p = 0usize;
    let take = |p: &mut usize, n: usize| -> Option<&[u8]> {
        let s = b.get(*p..*p + n)?;
        *p += n;
        Some(s)
    };
    let count = u32::from_le_bytes(take(&mut p, 4)?.try_into().ok()?) as usize;
    let mut map = HashMap::with_capacity(count);
    for _ in 0..count {
        let klen = u16::from_le_bytes(take(&mut p, 2)?.try_into().ok()?) as usize;
        let key = String::from_utf8(take(&mut p, klen)?.to_vec()).ok()?;
        let method = take(&mut p, 1)?[0];
        let offset = u32::from_le_bytes(take(&mut p, 4)?.try_into().ok()?);
        let comp = u32::from_le_bytes(take(&mut p, 4)?.try_into().ok()?);
        let raw = u32::from_le_bytes(take(&mut p, 4)?.try_into().ok()?);
        map.insert(key, Entry { method, offset, comp, raw });
    }
    Some(map)
}

pub fn key_from_path(path: &str) -> Option<String> {
    let norm = path.replace('\\', "/").to_ascii_lowercase();
    let i = norm.find("media/d3d11/majo/")?;
    Some(norm[i + "media/d3d11/majo/".len()..].to_string())
}

pub fn contains(key: &str) -> bool {
    PACK.get().is_some_and(|p| p.entries.contains_key(key))
}

pub fn size_of(key: &str) -> Option<u32> {
    PACK.get()?.entries.get(key).map(|e| e.raw)
}

pub fn read(key: &str) -> Option<Arc<Vec<u8>>> {
    let pack = PACK.get()?;
    let e = pack.entries.get(key)?;
    let mut comp = vec![0u8; e.comp as usize];
    if !read_at(pack.file as HANDLE, e.offset as u64, &mut comp) {
        return None;
    }
    let data = match e.method {
        0 => comp,
        1 => {
            let mut out = Vec::with_capacity(e.raw as usize);
            let mut dec = ruzstd::decoding::StreamingDecoder::new(&comp[..]).ok()?;
            dec.read_to_end(&mut out).ok()?;
            out
        }
        _ => return None,
    };
    if data.len() != e.raw as usize {
        return None;
    }
    Some(Arc::new(data))
}
