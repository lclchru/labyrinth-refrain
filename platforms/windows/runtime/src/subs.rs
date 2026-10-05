
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::{log, patch};

const DISPATCH: usize = 0x0052_40f0;
const STOLEN: [u8; 5] = [0x55, 0x8b, 0xec, 0x6a, 0xff];
const KIND_VOICE: i32 = 7;

static TRAMP: AtomicUsize = AtomicUsize::new(0);

struct State {
    lines: HashMap<String, (u64, String)>,
    current: Option<(String, u64)>,
    telop: Option<(String, u64)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn load_table() -> HashMap<String, (u64, String)> {
    let text = log::game_dir()
        .and_then(|d| std::fs::read_to_string(d.join("refrain-ru").join("subs.txt")).ok())
        .unwrap_or_else(|| include_str!("../assets/subs.txt").to_string());
    let mut out = HashMap::new();
    for raw in text.lines() {
        let raw = raw.trim_end();
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        let mut it = raw.splitn(4, '\t');
        let (Some(name), Some(sec), Some(body)) = (it.next(), it.next(), it.next()) else { continue };
        let Ok(sec) = sec.trim().parse::<f64>() else { continue };
        let ms = ((sec * 1000.0) as u64).max(1500);
        out.insert(name.trim().to_string(), (ms, body.trim().to_string()));
    }
    out
}

fn show(name: &str) {
    let Ok(mut g) = STATE.lock() else { return };
    let Some(st) = g.as_mut() else { return };
    if let Some((ms, text)) = st.lines.get(name) {
        let until = unsafe { GetTickCount64() } + ms;
        log::write(&format!("субтитр {name}: {text}"));
        st.current = Some((text.clone(), until));
    }
}

pub fn show_telop(text: &str, ms: u64) {
    let Ok(mut g) = STATE.lock() else { return };
    let Some(st) = g.as_mut() else { return };
    st.telop = Some((text.to_string(), unsafe { GetTickCount64() } + ms));
}

fn current_telop() -> Option<String> {
    let mut g = STATE.lock().ok()?;
    let st = g.as_mut()?;
    match &st.telop {
        Some((t, until)) if *until > unsafe { GetTickCount64() } => Some(t.clone()),
        Some(_) => {
            st.telop = None;
            None
        }
        None => None,
    }
}

fn current() -> Option<String> {
    let mut g = STATE.lock().ok()?;
    let st = g.as_mut()?;
    match &st.current {
        Some((t, until)) if *until > unsafe { GetTickCount64() } => Some(t.clone()),
        Some(_) => {
            st.current = None;
            None
        }
        None => None,
    }
}

#[no_mangle]
unsafe extern "C" fn refrain_on_sound(args: *const u8) {
    if args.is_null() {
        return;
    }
    let len = *(args.add(0x10) as *const u32) as usize;
    let cap = *(args.add(0x14) as *const u32) as usize;
    let kind = *(args.add(0x18) as *const i32);
    if kind != KIND_VOICE || len == 0 || len > 260 {
        return;
    }
    let p = if cap >= 16 { *(args as *const *const u8) } else { args };
    if p.is_null() {
        return;
    }
    if let Ok(name) = core::str::from_utf8(core::slice::from_raw_parts(p, len)) {
        show(name);
    }
}

unsafe fn make_stub() -> Option<usize> {
    use windows_sys::Win32::System::Memory::{VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE};
    let p = VirtualAlloc(core::ptr::null(), 64, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as *mut u8;
    if p.is_null() {
        return None;
    }
    let mut c: Vec<u8> = vec![0x60, 0x9c, 0x8d, 0x44, 0x24, 0x28, 0x50, 0xb8];
    c.extend((refrain_on_sound as *const () as usize as u32).to_le_bytes());
    c.extend([0xff, 0xd0, 0x83, 0xc4, 0x04, 0x9d, 0x61, 0xff, 0x25]);
    c.extend((&TRAMP as *const AtomicUsize as usize as u32).to_le_bytes());
    core::ptr::copy_nonoverlapping(c.as_ptr(), p, c.len());
    Some(p as usize)
}

pub fn install() {
    let lines = load_table();
    log::write(&format!("субтитров в таблице: {}", lines.len()));
    *STATE.lock().unwrap() = Some(State { lines, current: None, telop: None });
    unsafe {
        let Some(stub) = make_stub() else { return };
        match patch::hook(DISPATCH, &STOLEN, stub) {
            Ok(t) => {
                TRAMP.store(t, Ordering::Relaxed);
                log::write("звуковой диспетчер перехвачен");
            }
            Err(e) => {
                log::write(&format!("звуковой диспетчер не перехвачен: {e}"));
                return;
            }
        }
    }
    std::thread::spawn(overlay_thread);
}


static GAME_HWND: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn enum_cb(h: HWND, _: LPARAM) -> i32 {
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, &mut pid);
    if pid == GetCurrentProcessId() && IsWindowVisible(h) != 0 && GetWindow(h, GW_OWNER).is_null() {
        let mut r: RECT = core::mem::zeroed();
        GetClientRect(h, &mut r);
        if r.right - r.left > 320 {
            GAME_HWND.store(h as usize, Ordering::Relaxed);
            return 0;
        }
    }
    1
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: usize, l: isize) -> isize {
    if m == WM_NCHITTEST {
        return HTTRANSPARENT as isize;
    }
    DefWindowProcW(h, m, w, l)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn overlay_thread() {
    unsafe {
        static FONT: &[u8] = include_bytes!("../assets/subs.ttf");
        let mut n = 0u32;
        AddFontMemResourceEx(FONT.as_ptr() as _, FONT.len() as u32, core::ptr::null(), &mut n);

        let inst = GetModuleHandleW(core::ptr::null());
        let cls = wide("RefrainRuSubs");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: inst,
            lpszClassName: cls.as_ptr(),
            ..core::mem::zeroed()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            cls.as_ptr(),
            cls.as_ptr(),
            WS_POPUP,
            0, 0, 1, 1,
            core::ptr::null_mut(), core::ptr::null_mut(), inst, core::ptr::null(),
        );
        if hwnd.is_null() {
            log::write("окно субтитров не создано");
            return;
        }
        log::write("слой субтитров готов");
        let mut shown: Option<(String, i32, i32, i32, i32)> = None;
        let mut msg: MSG = core::mem::zeroed();
        loop {
            while PeekMessageW(&mut msg, core::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                DispatchMessageW(&msg);
            }
            std::thread::sleep(std::time::Duration::from_millis(33));

            let big = current_telop();
            let text = big.clone().or_else(current);
            let mut game = GAME_HWND.load(Ordering::Relaxed) as HWND;
            if game.is_null() || IsWindow(game) == 0 {
                GAME_HWND.store(0, Ordering::Relaxed);
                EnumWindows(Some(enum_cb), 0);
                game = GAME_HWND.load(Ordering::Relaxed) as HWND;
            }
            let fg = GetForegroundWindow();
            let Some(text) = text.filter(|_| !game.is_null() && IsIconic(game) == 0 && fg == game) else {
                if shown.take().is_some() {
                    ShowWindow(hwnd, SW_HIDE);
                }
                continue;
            };
            let mut r: RECT = core::mem::zeroed();
            GetClientRect(game, &mut r);
            let mut pt = POINT { x: 0, y: 0 };
            ClientToScreen(game, &mut pt);
            let (w, h) = (r.right, r.bottom);
            if w <= 0 || h <= 0 {
                continue;
            }
            let key = (format!("{}{text}", if big.is_some() { "!" } else { "" }), pt.x, pt.y, w, h);
            if shown.as_ref() != Some(&key) {
                render(hwnd, &text, pt.x, pt.y, w, h, big.is_some());
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                shown = Some(key);
            }
        }
    }
}

static TELOP_VICTORY: &[u8] = include_bytes!("../assets/telop_victory.bin");
static TELOP_LOSE: &[u8] = include_bytes!("../assets/telop_lose.bin");

unsafe fn blit_word(buf: &mut [u8], w: i32, h: i32, img: &[u8]) {
    let iw = u32::from_le_bytes(img[0..4].try_into().unwrap()) as i32;
    let ih = u32::from_le_bytes(img[4..8].try_into().unwrap()) as i32;
    let px = img.get(8..).unwrap_or(&[]);
    if iw <= 0 || ih <= 0 || px.len() < (iw * ih * 4) as usize {
        return;
    }
    let tw = ((w as f32 * 0.52) as i32).min(iw * h / 620);
    let th = (tw as i64 * ih as i64 / iw as i64) as i32;
    let (x0, y0) = ((w - tw) / 2, (h - th) / 2);
    for yy in 0..th {
        let sy = (yy as f32 + 0.5) * ih as f32 / th as f32 - 0.5;
        let (y1, fy) = (sy.floor().max(0.0) as i32, sy - sy.floor());
        for xx in 0..tw {
            let sx = (xx as f32 + 0.5) * iw as f32 / tw as f32 - 0.5;
            let (x1, fx) = (sx.floor().max(0.0) as i32, sx - sx.floor());
            let (dx, dy) = (x0 + xx, y0 + yy);
            if dx < 0 || dy < 0 || dx >= w || dy >= h {
                continue;
            }
            let di = ((dy * w + dx) * 4) as usize;
            for c in 0..4 {
                let at = |ix: i32, iy: i32| -> f32 {
                    let ix = ix.clamp(0, iw - 1);
                    let iy = iy.clamp(0, ih - 1);
                    px[((iy * iw + ix) * 4) as usize + c] as f32
                };
                let top = at(x1, y1) * (1.0 - fx) + at(x1 + 1, y1) * fx;
                let bot = at(x1, y1 + 1) * (1.0 - fx) + at(x1 + 1, y1 + 1) * fx;
                buf[di + c] = (top * (1.0 - fy) + bot * fy).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

unsafe fn render(hwnd: HWND, text: &str, x: i32, y: i32, w: i32, h: i32, big: bool) {
    let screen = GetDC(core::ptr::null_mut());
    let dc = CreateCompatibleDC(screen);
    let mut bi: BITMAPINFO = core::mem::zeroed();
    bi.bmiHeader.biSize = core::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
    let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, core::ptr::null_mut(), 0);
    let old = SelectObject(dc, bmp as _);

    if big {
        let img = if text.starts_with("ПОБ") { TELOP_VICTORY } else { TELOP_LOSE };
        let buf = core::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
        blit_word(buf, w, h, img);
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let dst = POINT { x, y };
        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        UpdateLayeredWindow(hwnd, screen, &dst, &size, dc, &src, 0, &blend, ULW_ALPHA);
        SelectObject(dc, old);
        DeleteObject(bmp as _);
        DeleteDC(dc);
        ReleaseDC(core::ptr::null_mut(), screen);
        return;
    }

    let px = h as f32 / 1080.0;
    let face = wide("EB Garamond");
    let font = CreateFontW(
        -(40.0 * px) as i32, 0, 0, 0, 500, 0, 0, 0,
        DEFAULT_CHARSET as u32, OUT_TT_PRECIS as u32, 0, ANTIALIASED_QUALITY as u32, 0,
        face.as_ptr(),
    );
    let oldf = SelectObject(dc, font as _);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, 0x00ff_ffff);

    let mut t: Vec<u16> = text.encode_utf16().collect();
    let wrap_w = ((1500.0 * px) as i32).min(w - 40);
    let mut calc = RECT { left: 0, top: 0, right: wrap_w, bottom: 0 };
    DrawTextW(dc, t.as_mut_ptr(), t.len() as i32, &mut calc, DT_CENTER | DT_WORDBREAK | DT_CALCRECT | DT_NOPREFIX);
    let tw = calc.right - calc.left;
    let th = calc.bottom - calc.top;
    let bottom = h - (96.0 * px) as i32;
    let mut tr = RECT { left: (w - tw) / 2, top: bottom - th, right: (w + tw) / 2, bottom };
    DrawTextW(dc, t.as_mut_ptr(), t.len() as i32, &mut tr, DT_CENTER | DT_WORDBREAK | DT_NOPREFIX);

    let pad = (16.0 * px) as i32;
    let (pl, pt_, pr, pb) = (tr.left - pad, tr.top - pad, tr.right + pad, tr.bottom + pad);
    let buf = core::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
    for yy in pt_.max(0)..pb.min(h) {
        for xx in pl.max(0)..pr.min(w) {
            let i = ((yy * w + xx) * 4) as usize;
            let cov = buf[i + 1] as u32;
            let a = 150 + (105 * cov) / 255;
            buf[i] = cov as u8;
            buf[i + 1] = cov as u8;
            buf[i + 2] = cov as u8;
            buf[i + 3] = a as u8;
        }
    }

    let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
    let dst = POINT { x, y };
    let size = SIZE { cx: w, cy: h };
    let src = POINT { x: 0, y: 0 };
    UpdateLayeredWindow(hwnd, screen, &dst, &size, dc, &src, 0, &blend, ULW_ALPHA);

    SelectObject(dc, oldf);
    DeleteObject(font as _);
    SelectObject(dc, old);
    DeleteObject(bmp as _);
    DeleteDC(dc);
    ReleaseDC(core::ptr::null_mut(), screen);
}
