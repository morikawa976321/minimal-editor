#![allow(non_snake_case, dead_code)]
use std::ffi::c_void;
pub type Handle = *mut c_void;
pub type Hwnd = Handle;
pub type WndProc = Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[repr(C)]
#[derive(Default)]
pub struct TextMetric {
    pub height: i32,
    pub ascent: i32,
    pub descent: i32,
    pub internal_leading: i32,
    pub external_leading: i32,
    pub average_width: i32,
    pub max_width: i32,
    pub weight: i32,
    pub overhang: i32,
    pub aspect_x: i32,
    pub aspect_y: i32,
    pub characters: [u16; 4],
    pub attributes: [u8; 5],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[repr(C)]
pub struct Msg {
    pub hwnd: Hwnd,
    pub message: u32,
    pub wparam: usize,
    pub lparam: isize,
    pub time: u32,
    pub pt: Point,
    pub private: u32,
}
#[repr(C)]
pub struct WndClass {
    pub style: u32,
    pub proc: WndProc,
    pub cls_extra: i32,
    pub wnd_extra: i32,
    pub instance: Handle,
    pub icon: Handle,
    pub cursor: Handle,
    pub background: Handle,
    pub menu: *const u16,
    pub name: *const u16,
}
#[repr(C)]
pub struct StartupInfo {
    pub size: u32,
    pub reserved: *mut u16,
    pub desktop: *mut u16,
    pub title: *mut u16,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub x_chars: u32,
    pub y_chars: u32,
    pub fill: u32,
    pub flags: u32,
    pub show: u16,
    pub reserved_size: u16,
    pub reserved_bytes: *mut u8,
    pub input: Handle,
    pub output: Handle,
    pub error: Handle,
}
#[repr(C)]
pub struct Paint {
    pub dc: Handle,
    pub erase: i32,
    pub rect: Rect,
    pub restore: i32,
    pub inc_update: i32,
    pub reserved: [u8; 32],
}
#[repr(C)]
#[derive(Default)]
pub struct CharRange {
    pub min: i32,
    pub max: i32,
}
// CHARFORMAT2W extends the complete 92-byte CHARFORMATW base. That base has
// two trailing padding bytes, even though flattening the fields hides them.
#[repr(C)]
pub struct CharFormat {
    pub size: u32,
    pub mask: u32,
    pub effects: u32,
    pub height: i32,
    pub offset: i32,
    pub color: u32,
    pub charset: u8,
    pub pitch: u8,
    pub face: [u16; 32],
    pub base_pad: u16,
    pub weight: u16,
    pub spacing: i16,
    pub bg: u32,
    pub locale: u32,
    pub reserved: u32,
    pub style: i16,
    pub kerning: u16,
    pub underline: u8,
    pub animation: u8,
    pub author: u8,
    pub reserved2: u8,
}
#[repr(C)]
pub struct ParaFormat {
    pub size: u32,
    pub mask: u32,
    pub numbering: u16,
    pub reserved: u16,
    pub start: i32,
    pub right: i32,
    pub offset: i32,
    pub align: u16,
    pub tabs: i16,
    pub tabstops: [i32; 32],
    pub space_before: i32,
    pub space_after: i32,
    pub line_spacing: i32,
    pub style: i16,
    pub line_rule: u8,
    pub outline: u8,
    pub shading_weight: u16,
    pub shading_style: u16,
    pub numbering_start: u16,
    pub numbering_style: u16,
    pub numbering_tab: u16,
    pub border_space: u16,
    pub border_width: u16,
    pub borders: u16,
}
#[repr(C)]
pub struct ScrollInfo {
    pub size: u32,
    pub mask: u32,
    pub min: i32,
    pub max: i32,
    pub page: u32,
    pub pos: i32,
    pub track: i32,
}
#[repr(C)]
pub struct TrackMouse {
    pub size: u32,
    pub flags: u32,
    pub hwnd: Hwnd,
    pub hover: u32,
}
#[repr(C, packed(4))]
pub struct EditStream {
    pub cookie: usize,
    pub error: u32,
    pub callback: Option<unsafe extern "system" fn(usize, *mut u8, i32, *mut i32) -> u32>,
}
// The SDK's C++ base structure and Rust's flattened repr(C) differ unless its
// tail padding is explicit: see Microsoft windows-rs CHARFORMAT2W::Base.
const _: () = {
    assert!(std::mem::size_of::<TextMetric>() == 60);
    assert!(std::mem::size_of::<CharFormat>() == 116);
    assert!(std::mem::offset_of!(CharFormat, weight) == 92);
    assert!(std::mem::offset_of!(CharFormat, spacing) == 94);
    assert!(std::mem::size_of::<ParaFormat>() == 188);
    assert!(std::mem::offset_of!(ParaFormat, line_rule) == 170);
    assert!(std::mem::size_of::<EditStream>() == 2 * std::mem::size_of::<usize>() + 4);
    assert!(std::mem::offset_of!(EditStream, callback) == std::mem::size_of::<usize>() + 4);
};
#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassW(c: *const WndClass) -> u16;
    pub fn CreateWindowExW(
        ex: u32,
        cls: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Hwnd,
        menu: Handle,
        instance: Handle,
        param: Handle,
    ) -> Hwnd;
    pub fn DefWindowProcW(h: Hwnd, m: u32, w: usize, l: isize) -> isize;
    pub fn CallWindowProcW(proc: WndProc, h: Hwnd, m: u32, w: usize, l: isize) -> isize;
    pub fn SetWindowLongPtrW(h: Hwnd, index: i32, value: isize) -> isize;
    pub fn GetMessageW(m: *mut Msg, h: Hwnd, min: u32, max: u32) -> i32;
    pub fn TranslateMessage(m: *const Msg) -> i32;
    pub fn DispatchMessageW(m: *const Msg) -> isize;
    pub fn ShowWindow(h: Hwnd, n: i32) -> i32;
    pub fn ShowWindowAsync(h: Hwnd, n: i32) -> i32;
    pub fn IsWindowVisible(h: Hwnd) -> i32;
    pub fn EnableWindow(h: Hwnd, yes: i32) -> i32;
    pub fn SetFocus(h: Hwnd) -> Hwnd;
    pub fn GetFocus() -> Hwnd;
    pub fn GetCursorPos(point: *mut Point) -> i32;
    pub fn ScreenToClient(h: Hwnd, point: *mut Point) -> i32;
    pub fn SystemParametersInfoW(
        action: u32,
        parameter: u32,
        value: *mut c_void,
        flags: u32,
    ) -> i32;
    pub fn SetForegroundWindow(h: Hwnd) -> i32;
    pub fn DestroyWindow(h: Hwnd) -> i32;
    pub fn PostQuitMessage(n: i32);
    pub fn SendMessageW(h: Hwnd, m: u32, w: usize, l: isize) -> isize;
    pub fn PostMessageW(h: Hwnd, m: u32, w: usize, l: isize) -> i32;
    pub fn SetWindowTextW(h: Hwnd, s: *const u16) -> i32;
    pub fn GetWindowTextW(h: Hwnd, s: *mut u16, n: i32) -> i32;
    pub fn GetWindowTextLengthW(h: Hwnd) -> i32;
    pub fn GetClientRect(h: Hwnd, r: *mut Rect) -> i32;
    pub fn MoveWindow(h: Hwnd, x: i32, y: i32, w: i32, height: i32, repaint: i32) -> i32;
    pub fn SetWindowPos(
        h: Hwnd,
        after: Hwnd,
        x: i32,
        y: i32,
        w: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    pub fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    pub fn GetKeyState(key: i32) -> i16;
    pub fn FindWindowW(class: *const u16, title: *const u16) -> Hwnd;
    pub fn GetDpiForWindow(h: Hwnd) -> u32;
    pub fn SetProcessDpiAwarenessContext(c: Handle) -> i32;
    pub fn InvalidateRect(h: Hwnd, r: *const Rect, erase: i32) -> i32;
    pub fn BeginPaint(h: Hwnd, p: *mut Paint) -> Handle;
    pub fn EndPaint(h: Hwnd, p: *const Paint) -> i32;
    pub fn FillRect(dc: Handle, r: *const Rect, brush: Handle) -> i32;
    pub fn DrawTextW(dc: Handle, text: *const u16, len: i32, r: *mut Rect, flags: u32) -> i32;
    pub fn SetTimer(h: Hwnd, id: usize, ms: u32, proc: Handle) -> usize;
    pub fn KillTimer(h: Hwnd, id: usize) -> i32;
    pub fn MessageBoxW(h: Hwnd, text: *const u16, title: *const u16, kind: u32) -> i32;
    pub fn GetScrollInfo(h: Hwnd, bar: i32, info: *mut ScrollInfo) -> i32;
    pub fn SetCapture(h: Hwnd) -> Hwnd;
    pub fn ReleaseCapture() -> i32;
    pub fn TrackMouseEvent(t: *mut TrackMouse) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> Handle;
    pub fn LoadLibraryW(name: *const u16) -> Handle;
    pub fn GetStartupInfoW(info: *mut StartupInfo);
}
#[link(name = "gdi32")]
extern "system" {
    pub fn GdiFlush() -> i32;
    pub fn CreateCompatibleDC(dc: Handle) -> Handle;
    pub fn CreateCompatibleBitmap(dc: Handle, width: i32, height: i32) -> Handle;
    pub fn DeleteDC(dc: Handle) -> i32;
    pub fn SetViewportOrgEx(dc: Handle, x: i32, y: i32, previous: *mut Point) -> i32;
    pub fn BitBlt(
        destination: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Handle,
        source_x: i32,
        source_y: i32,
        operation: u32,
    ) -> i32;
    pub fn CreateSolidBrush(color: u32) -> Handle;
    pub fn DeleteObject(obj: Handle) -> i32;
    pub fn SelectObject(dc: Handle, obj: Handle) -> Handle;
    pub fn GetTextMetricsW(dc: Handle, metrics: *mut TextMetric) -> i32;
    pub fn GetTextExtentPoint32W(dc: Handle, text: *const u16, count: i32, size: *mut Point)
        -> i32;
    pub fn SetTextColor(dc: Handle, color: u32) -> u32;
    pub fn SetBkMode(dc: Handle, mode: i32) -> i32;
    pub fn SetBkColor(dc: Handle, color: u32) -> u32;
    pub fn CreateFontW(
        height: i32,
        width: i32,
        esc: i32,
        orient: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        out: u32,
        clip: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> Handle;
    pub fn RoundRect(
        dc: Handle,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        w: i32,
        h: i32,
    ) -> i32;
    pub fn GetStockObject(id: i32) -> Handle;
}
#[link(name = "dwmapi")]
extern "system" {
    pub fn DwmSetWindowAttribute(h: Hwnd, attribute: u32, value: *const c_void, size: u32) -> i32;
}

pub const WM_KEYDOWN: u32 = 0x100;
pub const WM_CHAR: u32 = 0x102;
pub const EM_SETCHARFORMAT: u32 = 0x444;
pub const EM_GETCHARFORMAT: u32 = 0x43a;
pub const EM_SETPARAFORMAT: u32 = 0x447;
pub const EM_GETPARAFORMAT: u32 = 0x43d;
pub const EM_GETSCROLLPOS: u32 = 0x4dd;
pub const EM_SETSCROLLPOS: u32 = 0x4de;
pub const EM_EXGETSEL: u32 = 0x434;
pub const EM_EXSETSEL: u32 = 0x437;
