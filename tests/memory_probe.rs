//! Small architecture probe; not a usable editor or product memory benchmark.
//! rustc --edition=2021 -O tests/memory_probe.rs -o build/memory-probe.exe
//! --mode bare|edit|richedit|console-worker --hold-ms 15000 --data-dir <test-dir>
//! Reads probe.txt and writes ready.json in the explicit test directory.
//! Run GUI modes only on the test harness's private, never displayed desktop.
#![windows_subsystem = "windows"]
#![allow(non_snake_case)]

use std::{
    ffi::c_void,
    fs, io,
    path::{Path, PathBuf},
    ptr,
    sync::OnceLock,
    time::Duration,
};
type Handle = *mut c_void;
type Hwnd = Handle;
#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    pt: Point,
    private: u32,
}
type WindowProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize;
#[repr(C)]
struct WndClass {
    style: u32,
    proc: Option<WindowProc>,
    cls_extra: i32,
    wnd_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu: *const u16,
    name: *const u16,
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn LoadLibraryW(name: *const u16) -> Handle;
    fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
}

// Resolve GUI APIs only inside the GUI branch. Otherwise console-worker would
// load user32/gdi32 before main and cease to be a meaningful no-GUI baseline.
struct Api {
    RegisterClassW: unsafe extern "system" fn(*const WndClass) -> u16,
    CreateWindowExW: unsafe extern "system" fn(
        u32,
        *const u16,
        *const u16,
        u32,
        i32,
        i32,
        i32,
        i32,
        Hwnd,
        Handle,
        Handle,
        Handle,
    ) -> Hwnd,
    DefWindowProcW: WindowProc,
    ShowWindow: unsafe extern "system" fn(Hwnd, i32) -> i32,
    UpdateWindow: unsafe extern "system" fn(Hwnd) -> i32,
    SetFocus: unsafe extern "system" fn(Hwnd) -> Hwnd,
    GetMessageW: unsafe extern "system" fn(*mut Msg, Hwnd, u32, u32) -> i32,
    TranslateMessage: unsafe extern "system" fn(*const Msg) -> i32,
    DispatchMessageW: unsafe extern "system" fn(*const Msg) -> isize,
    PostQuitMessage: unsafe extern "system" fn(i32),
    DestroyWindow: unsafe extern "system" fn(Hwnd) -> i32,
    SendMessageW: unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize,
    SetWindowTextW: unsafe extern "system" fn(Hwnd, *const u16) -> i32,
    SetTimer: unsafe extern "system" fn(Hwnd, usize, u32, *const c_void) -> usize,
    KillTimer: unsafe extern "system" fn(Hwnd, usize) -> i32,
    SetProcessDPIAware: unsafe extern "system" fn() -> i32,
    BeginPaint: unsafe extern "system" fn(Hwnd, *mut Paint) -> Handle,
    EndPaint: unsafe extern "system" fn(Hwnd, *const Paint) -> i32,
    DrawTextW: unsafe extern "system" fn(Handle, *const u16, i32, *mut Rect, u32) -> i32,
    CreateSolidBrush: unsafe extern "system" fn(u32) -> Handle,
    SetTextColor: unsafe extern "system" fn(Handle, u32) -> u32,
    SetBkColor: unsafe extern "system" fn(Handle, u32) -> u32,
    SelectObject: unsafe extern "system" fn(Handle, Handle) -> Handle,
    CreateFontW: unsafe extern "system" fn(
        i32,
        i32,
        i32,
        i32,
        i32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        *const u16,
    ) -> Handle,
    DeleteObject: unsafe extern "system" fn(Handle) -> i32,
}
#[repr(C)]
struct Paint {
    dc: Handle,
    erase: i32,
    rect: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}
#[repr(C)]
struct CharFormat {
    size: u32,
    mask: u32,
    effects: u32,
    height: i32,
    offset: i32,
    color: u32,
    charset: u8,
    pitch: u8,
    face: [u16; 32],
}
static API: OnceLock<Api> = OnceLock::new();
struct State {
    brush: Handle,
    font: Handle,
    text: Vec<u16>,
    bare: bool,
}
static mut STATE: *mut State = ptr::null_mut();
const BG: u32 = 0x202020;
const FG: u32 = 0xdfdfdf;
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn load_api() -> io::Result<Api> {
    let user = LoadLibraryW(wide("user32.dll").as_ptr());
    if user.is_null() {
        return Err(io::Error::last_os_error());
    }
    let gdi = LoadLibraryW(wide("gdi32.dll").as_ptr());
    if gdi.is_null() {
        return Err(io::Error::last_os_error());
    }
    macro_rules! get {
        ($module:expr,$name:ident,$ty:ty) => {{
            let p = GetProcAddress($module, concat!(stringify!($name), "\0").as_ptr());
            if p.is_null() {
                return Err(io::Error::last_os_error());
            }
            std::mem::transmute::<*mut c_void, $ty>(p)
        }};
    }
    Ok(Api {
        RegisterClassW: get!(
            user,
            RegisterClassW,
            unsafe extern "system" fn(*const WndClass) -> u16
        ),
        CreateWindowExW: get!(
            user,
            CreateWindowExW,
            unsafe extern "system" fn(
                u32,
                *const u16,
                *const u16,
                u32,
                i32,
                i32,
                i32,
                i32,
                Hwnd,
                Handle,
                Handle,
                Handle,
            ) -> Hwnd
        ),
        DefWindowProcW: get!(user, DefWindowProcW, WindowProc),
        ShowWindow: get!(
            user,
            ShowWindow,
            unsafe extern "system" fn(Hwnd, i32) -> i32
        ),
        UpdateWindow: get!(user, UpdateWindow, unsafe extern "system" fn(Hwnd) -> i32),
        SetFocus: get!(user, SetFocus, unsafe extern "system" fn(Hwnd) -> Hwnd),
        GetMessageW: get!(
            user,
            GetMessageW,
            unsafe extern "system" fn(*mut Msg, Hwnd, u32, u32) -> i32
        ),
        TranslateMessage: get!(
            user,
            TranslateMessage,
            unsafe extern "system" fn(*const Msg) -> i32
        ),
        DispatchMessageW: get!(
            user,
            DispatchMessageW,
            unsafe extern "system" fn(*const Msg) -> isize
        ),
        PostQuitMessage: get!(user, PostQuitMessage, unsafe extern "system" fn(i32)),
        DestroyWindow: get!(user, DestroyWindow, unsafe extern "system" fn(Hwnd) -> i32),
        SendMessageW: get!(
            user,
            SendMessageW,
            unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize
        ),
        SetWindowTextW: get!(
            user,
            SetWindowTextW,
            unsafe extern "system" fn(Hwnd, *const u16) -> i32
        ),
        SetTimer: get!(
            user,
            SetTimer,
            unsafe extern "system" fn(Hwnd, usize, u32, *const c_void) -> usize
        ),
        KillTimer: get!(
            user,
            KillTimer,
            unsafe extern "system" fn(Hwnd, usize) -> i32
        ),
        SetProcessDPIAware: get!(user, SetProcessDPIAware, unsafe extern "system" fn() -> i32),
        BeginPaint: get!(
            user,
            BeginPaint,
            unsafe extern "system" fn(Hwnd, *mut Paint) -> Handle
        ),
        EndPaint: get!(
            user,
            EndPaint,
            unsafe extern "system" fn(Hwnd, *const Paint) -> i32
        ),
        DrawTextW: get!(
            user,
            DrawTextW,
            unsafe extern "system" fn(Handle, *const u16, i32, *mut Rect, u32) -> i32
        ),
        CreateSolidBrush: get!(
            gdi,
            CreateSolidBrush,
            unsafe extern "system" fn(u32) -> Handle
        ),
        SetTextColor: get!(
            gdi,
            SetTextColor,
            unsafe extern "system" fn(Handle, u32) -> u32
        ),
        SetBkColor: get!(
            gdi,
            SetBkColor,
            unsafe extern "system" fn(Handle, u32) -> u32
        ),
        SelectObject: get!(
            gdi,
            SelectObject,
            unsafe extern "system" fn(Handle, Handle) -> Handle
        ),
        CreateFontW: get!(
            gdi,
            CreateFontW,
            unsafe extern "system" fn(
                i32,
                i32,
                i32,
                i32,
                i32,
                u32,
                u32,
                u32,
                u32,
                u32,
                u32,
                u32,
                u32,
                *const u16,
            ) -> Handle
        ),
        DeleteObject: get!(gdi, DeleteObject, unsafe extern "system" fn(Handle) -> i32),
    })
}

unsafe extern "system" fn window_proc(h: Hwnd, m: u32, w: usize, l: isize) -> isize {
    let a = API.get().unwrap();
    match m {
        2 => {
            (a.PostQuitMessage)(0);
            0
        }
        0x113 => {
            (a.PostQuitMessage)(0);
            0
        }
        0x133 if !STATE.is_null() => {
            (a.SetTextColor)(w as Handle, FG);
            (a.SetBkColor)(w as Handle, BG);
            (*STATE).brush as isize
        }
        0x0f if !STATE.is_null() && (*STATE).bare => {
            let mut ps: Paint = std::mem::zeroed();
            let dc = (a.BeginPaint)(h, &mut ps);
            let old = (a.SelectObject)(dc, (*STATE).font);
            (a.SetTextColor)(dc, FG);
            (a.SetBkColor)(dc, BG);
            let mut rect = Rect {
                left: 48,
                top: 60,
                right: 896,
                bottom: 660,
            };
            (a.DrawTextW)(dc, (*STATE).text.as_ptr(), -1, &mut rect, 0x810);
            (a.SelectObject)(dc, old);
            (a.EndPaint)(h, &ps);
            0
        }
        _ => (a.DefWindowProcW)(h, m, w, l),
    }
}

fn ready(dir: &Path, mode: &str, chars: usize, h: Hwnd, edit: Hwnd) -> io::Result<()> {
    fs::write(dir.join("ready.json"),format!(
        "{{\"mode\":\"{mode}\",\"pid\":{},\"text_characters\":{chars},\"window\":{},\"editor\":{}}}",
        std::process::id(),h as usize,edit as usize))
}

unsafe fn gui(dir: &Path, mode: &str, hold: u32, text: Vec<u16>, chars: usize) -> io::Result<()> {
    API.set(load_api()?)
        .map_err(|_| io::Error::other("API already initialized"))?;
    let a = API.get().unwrap();
    (a.SetProcessDPIAware)();
    if mode == "richedit" && LoadLibraryW(wide("msftedit.dll").as_ptr()).is_null() {
        return Err(io::Error::last_os_error());
    }
    let brush = (a.CreateSolidBrush)(BG);
    let font = (a.CreateFontW)(
        -20,
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        1,
        0,
        0,
        5,
        0,
        wide("BIZ UDGothic").as_ptr(),
    );
    if brush.is_null() || font.is_null() {
        return Err(io::Error::last_os_error());
    }
    STATE = Box::into_raw(Box::new(State {
        brush,
        font,
        text,
        bare: mode == "bare",
    }));
    let instance = GetModuleHandleW(ptr::null());
    let class = wide("MinimalEditorMemoryProbe");
    let wc = WndClass {
        style: 3,
        proc: Some(window_proc),
        cls_extra: 0,
        wnd_extra: 0,
        instance,
        icon: ptr::null_mut(),
        cursor: ptr::null_mut(),
        background: brush,
        menu: ptr::null(),
        name: class.as_ptr(),
    };
    if (a.RegisterClassW)(&wc) == 0 {
        return Err(io::Error::last_os_error());
    }
    let h = (a.CreateWindowExW)(
        0,
        class.as_ptr(),
        class.as_ptr(),
        0x02cf0000,
        i32::MIN,
        i32::MIN,
        960,
        740,
        ptr::null_mut(),
        ptr::null_mut(),
        instance,
        ptr::null_mut(),
    );
    if h.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut edit = ptr::null_mut();
    if mode != "bare" {
        let cls = wide(if mode == "edit" {
            "EDIT"
        } else {
            "RICHEDIT50W"
        });
        edit = (a.CreateWindowExW)(
            0,
            cls.as_ptr(),
            wide("").as_ptr(),
            0x50000044,
            48,
            60,
            848,
            600,
            h,
            ptr::null_mut(),
            instance,
            ptr::null_mut(),
        );
        if edit.is_null() {
            return Err(io::Error::last_os_error());
        }
        if mode == "richedit" {
            if (a.SendMessageW)(edit, 0x459, 1 | 8 | 32, 0) != 0 {
                return Err(io::Error::other("EM_SETTEXTMODE rejected"));
            }
            (a.SendMessageW)(edit, 0x443, 0, BG as isize); // EM_SETBKGNDCOLOR
            (a.SendMessageW)(edit, 0x435, 0, 16_777_216); // EM_EXLIMITTEXT
        } else {
            (a.SendMessageW)(edit, 0xc5, 16_777_216, 0);
        }
        (a.SendMessageW)(edit, 0x30, font as usize, 1); // WM_SETFONT
        if mode == "richedit" {
            let mut format: CharFormat = std::mem::zeroed();
            format.size = std::mem::size_of::<CharFormat>() as u32;
            format.mask = 0x40000000;
            format.color = FG;
            (a.SendMessageW)(edit, 0x444, 0, &format as *const CharFormat as isize);
        }
        if (a.SetWindowTextW)(edit, (*STATE).text.as_ptr()) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    if (a.SetTimer)(h, 1, hold, ptr::null()) == 0 {
        return Err(io::Error::last_os_error());
    }
    (a.ShowWindow)(h, 4); // SW_SHOWNOACTIVATE; private desktop remains invisible.
    if !edit.is_null() {
        (a.SetFocus)(edit);
    }
    (a.UpdateWindow)(h);
    if !edit.is_null() {
        (a.UpdateWindow)(edit);
    }
    ready(dir, mode, chars, h, edit)?;
    let mut msg: Msg = std::mem::zeroed();
    loop {
        let result = (a.GetMessageW)(&mut msg, ptr::null_mut(), 0, 0);
        if result == 0 {
            break;
        }
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        (a.TranslateMessage)(&msg);
        (a.DispatchMessageW)(&msg);
    }
    (a.KillTimer)(h, 1);
    (a.DestroyWindow)(h);
    let state = Box::from_raw(STATE);
    STATE = ptr::null_mut();
    (a.DeleteObject)(state.font);
    (a.DeleteObject)(state.brush);
    Ok(())
}

fn run() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut mode, mut hold, mut dir) = (String::from("bare"), 15_000u32, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                mode = args
                    .next()
                    .ok_or_else(|| io::Error::other("Missing mode"))?
            }
            "--hold-ms" => {
                hold = args
                    .next()
                    .ok_or_else(|| io::Error::other("Missing hold"))?
                    .parse()
                    .map_err(|_| io::Error::other("Invalid hold"))?
            }
            "--data-dir" => {
                dir = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| io::Error::other("Missing data dir"))?,
                ))
            }
            _ => return Err(io::Error::other("Unknown argument")),
        }
    }
    if !["bare", "edit", "richedit", "console-worker"].contains(&mode.as_str()) || hold < 1000 {
        return Err(io::Error::other("Invalid mode or hold below 1000 ms"));
    }
    let dir = dir.ok_or_else(|| io::Error::other("Explicit test --data-dir is required"))?;
    fs::create_dir_all(&dir)?;
    let text = match fs::read_to_string(dir.join("probe.txt")) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let chars = text.chars().count();
    let control = wide(
        &text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\r\n"),
    );
    drop(text);
    if mode == "console-worker" {
        ready(&dir, &mode, chars, ptr::null_mut(), ptr::null_mut())?;
        std::thread::sleep(Duration::from_millis(hold.into()));
        std::hint::black_box(&control);
    } else {
        unsafe {
            gui(&dir, &mode, hold, control, chars)?;
        }
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
