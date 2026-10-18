//! Native comparison prototype: std-only Rust, system RichEdit, local UTF-8 memos.
//! --probe and --test-mode always require --data-dir. No global input is sent.
#![windows_subsystem = "windows"]
#![allow(non_snake_case)]
mod store;
mod win;
mod worker;
use std::{
    ffi::c_void,
    fs,
    hash::{Hash, Hasher},
    mem,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant},
};
use store::{Memo, Store};
use win::*;
use worker::{Action, Request, View, Worker};

const BG: u32 = 0x202020;
const FG: u32 = 0xdfdfdf;
const PANEL: u32 = 0x292929;
const SELECTED: u32 = 0x414141;
const TITLE: &str = "minimal-editor · Native";
const TEST_SHORTCUT: u32 = 0x8001;
const TEST_SNAPSHOT: u32 = 0x8002;
const TEST_CLOSE: u32 = 0x8003;
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn normalized(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}
fn control_text(s: &str) -> String {
    normalized(s).replace('\n', "\r\n")
}
fn json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn err() -> String {
    std::io::Error::last_os_error().to_string()
}

enum PendingAction {
    Storage(Action),
    Close,
}
struct State {
    worker: Worker,
    view: View,
    inflight: bool,
    saving: bool,
    action_inflight: bool,
    close_after_action: bool,
    pending_action: Option<PendingAction>,
    worker_failed: bool,
    generation: u64,
    saved_generation: u64,
    last_save_ms: f64,
    window: Hwnd,
    body: Hwnd,
    overlay: Hwnd,
    title_edit: Hwnd,
    thumb: Hwnd,
    body_proc: WndProc,
    title_proc: WndProc,
    brush: Handle,
    font: Handle,
    small_font: Handle,
    active: Option<String>,
    dirty: bool,
    loading: bool,
    ready: bool,
    hidden: bool,
    test_mode: bool,
    report: Option<PathBuf>,
    error: Option<String>,
    picker: Option<bool>,
    items: Vec<Memo>,
    selected: usize,
    first: usize,
    renaming: bool,
    composing: bool,
    rename_error: Option<String>,
    undo_archive: Option<String>,
    toast_deadline: Option<Instant>,
    toast_hover: bool,
    toast_focus: bool,
    picker_rect: Rect,
    toast_rect: Rect,
    row_height: i32,
    scale: f64,
    selection: CharRange,
    scroll: Point,
    restore_position: bool,
    thumb_visible: bool,
    thumb_hover: bool,
    thumb_drag: Option<(i32, i32)>,
    thumb_rect: Rect,
    ready_ms: f64,
    save_count: u64,
    recovery: Option<PathBuf>,
    paragraph_applied: bool,
}
static mut STATE: *mut State = ptr::null_mut();
unsafe fn px(s: *mut State, v: i32) -> i32 {
    (v as f64 * (*s).scale).round() as i32
}
unsafe fn read_text(h: Hwnd) -> Result<String, String> {
    let len = GetWindowTextLengthW(h);
    if len < 0 {
        return Err("本文の長さを取得できません".into());
    }
    let mut buf = vec![0u16; len as usize + 1];
    let got = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
    if got != len {
        return Err("本文をすべて取得できません。画面の本文を保持しています".into());
    }
    String::from_utf16(&buf[..got as usize])
        .map(|t| normalized(&t))
        .map_err(|_| "入力の文字が確定していません".into())
}
unsafe fn update_caption(s: *mut State) {
    let title = if (*s).error.is_some() {
        "未保存 · 本文はこの画面に残っています · Native"
    } else {
        TITLE
    };
    SetWindowTextW((*s).window, wide(title).as_ptr());
    InvalidateRect((*s).window, ptr::null(), 0);
}
unsafe fn fail(s: *mut State, message: String) {
    (*s).error = Some(message);
    update_caption(s);
    snapshot(s);
}
// Only the worker touches memo files after startup. The UI owns the latest
// buffer, and queues at most one request; edits during I/O stay in RichEdit.
unsafe fn save(s: *mut State) -> bool {
    if (*s).inflight || (*s).composing || (*s).worker_failed {
        return false;
    }
    if !(*s).dirty {
        return true;
    }
    let Some(id) = (*s).active.clone() else {
        return true;
    };
    let text = match read_text((*s).body) {
        Ok(text) => text,
        Err(e) => {
            fail(s, e);
            return false;
        }
    };
    let request = Request::Save {
        id,
        text,
        generation: (*s).generation,
    };
    match (*s).worker.send(request) {
        Ok(()) => {
            (*s).inflight = true;
            (*s).saving = true;
            SetTimer((*s).window, 5, 15, ptr::null_mut());
        }
        Err(e) => storage_disconnected(s, e),
    }
    false
}
unsafe fn storage_disconnected(s: *mut State, error: String) {
    KillTimer((*s).window, 5);
    (*s).worker_failed = true;
    (*s).inflight = false;
    (*s).saving = false;
    (*s).action_inflight = false;
    (*s).close_after_action = false;
    (*s).pending_action = None;
    SendMessageW((*s).body, 0xcf, usize::from((*s).active.is_none()), 0);
    EnableWindow((*s).title_edit, 1);
    fail(s, error);
}
unsafe fn busy(s: *mut State) -> bool {
    (*s).pending_action.is_some() || (*s).action_inflight
}
unsafe fn request_action(s: *mut State, action: PendingAction) {
    if matches!(action, PendingAction::Close) {
        if (*s).action_inflight {
            (*s).close_after_action = true;
            return;
        }
        // Closing supersedes a not-yet-started switch/new request, while the
        // current document still goes through its final save barrier.
        (*s).pending_action = None;
    }
    if busy(s) || (*s).worker_failed {
        return;
    }
    (*s).pending_action = Some(action);
    SetTimer((*s).window, 5, 15, ptr::null_mut());
    continue_action(s);
    snapshot(s);
}
unsafe fn continue_action(s: *mut State) {
    if (*s).pending_action.is_none() || !save(s) {
        return;
    }
    match (*s).pending_action.take().unwrap() {
        PendingAction::Close => {
            DestroyWindow((*s).window);
        }
        PendingAction::Storage(action) => {
            match (*s).worker.send(Request::Action(action)) {
                Ok(()) => {
                    (*s).inflight = true;
                    (*s).action_inflight = true;
                    SetTimer((*s).window, 5, 15, ptr::null_mut());
                    // A document replacement cannot race new edits. Normal
                    // autosave never makes the editor read-only.
                    SendMessageW((*s).body, 0xcf, 1, 0);
                    EnableWindow((*s).title_edit, 0);
                }
                Err(e) => storage_disconnected(s, e),
            }
        }
    }
}
unsafe fn poll_storage(s: *mut State) {
    if (*s).worker_failed {
        return;
    }
    let reply = match (*s).worker.poll() {
        Ok(Some(reply)) => reply,
        Ok(None) => return,
        Err(e) => {
            storage_disconnected(s, e);
            return;
        }
    };
    (*s).inflight = false;
    (*s).saving = false;
    (*s).view = reply.view;
    match reply.request {
        Request::Save { id, generation, .. } => {
            (*s).last_save_ms = reply.save_ms;
            if let Some(error) = reply.error {
                (*s).close_after_action = false;
                (*s).dirty = true;
                (*s).pending_action = None;
                (*s).recovery = reply.recovery;
                let note = (*s)
                    .recovery
                    .as_ref()
                    .map(|p| format!("\n復元用コピー: {}", p.display()))
                    .unwrap_or_default();
                fail(s, format!("{error}{note}"));
                // Preserve a newer buffer too, rather than only recovering the
                // older text that happened to be in flight when the error hit.
                if (*s).active.as_deref() == Some(&id) && (*s).generation > generation {
                    save(s);
                }
            } else {
                (*s).save_count += 1;
                (*s).saved_generation = generation;
                if (*s).active.as_deref() == Some(&id) && (*s).generation == generation {
                    (*s).dirty = false;
                    (*s).error = None;
                    (*s).recovery = None;
                }
                update_caption(s);
                if (*s).picker.is_some() && !(*s).renaming {
                    let preferred = (&(*s).items).get((*s).selected).map(|m| m.id.clone());
                    reload_items(s, preferred.as_deref());
                }
                if (*s).dirty {
                    save(s);
                }
                continue_action(s);
            }
        }
        Request::Action(action) => {
            (*s).action_inflight = false;
            SendMessageW((*s).body, 0xcf, usize::from((*s).active.is_none()), 0);
            EnableWindow((*s).title_edit, 1);
            if let Some(error) = reply.error {
                (*s).close_after_action = false;
                if matches!(action, Action::Rename { .. }) {
                    (*s).rename_error = Some(error);
                    InvalidateRect((*s).overlay, ptr::null(), 0);
                    SetFocus((*s).title_edit);
                } else {
                    fail(s, error);
                }
            } else {
                if let Some((id, text)) = reply.document {
                    if let Err(error) = load_text(s, id, &text) {
                        fail(s, error);
                        snapshot(s);
                        return;
                    }
                }
                (*s).error = None;
                match action {
                    Action::Create | Action::Switch { .. } => close_picker(s),
                    Action::Archive { id } => {
                        (*s).undo_archive = Some(id);
                        (*s).toast_deadline = Some(Instant::now() + Duration::from_secs(8));
                        SetTimer((*s).window, 2, 100, ptr::null_mut());
                        reload_items(s, None);
                        SetFocus((*s).overlay);
                    }
                    Action::Restore { id } => {
                        (*s).picker = Some(false);
                        hide_toast(s);
                        reload_items(s, Some(&id));
                        SetFocus((*s).overlay);
                    }
                    Action::Rename { id, .. } => {
                        cancel_rename(s);
                        reload_items(s, Some(&id));
                        SetFocus((*s).overlay);
                    }
                }
                update_caption(s);
                if (*s).close_after_action {
                    (*s).close_after_action = false;
                    request_action(s, PendingAction::Close);
                }
            }
        }
    }
    if !(*s).inflight && (*s).pending_action.is_none() {
        KillTimer((*s).window, 5);
    }
    snapshot(s);
}
unsafe fn load_text(s: *mut State, id: Option<String>, text: &str) -> Result<(), String> {
    // Read the complete target before this function. WM_SETTEXT is reversible if
    // RichEdit rejects it; catalog activation must already have succeeded.
    let previous = read_text((*s).body)?;
    let old_id = (*s).active.clone();
    (*s).loading = true;
    let accepted = set_literal_text((*s).body, text);
    let verified = read_text((*s).body)
        .map(|v| v == normalized(text))
        .unwrap_or(false);
    if !accepted || !verified {
        set_literal_text((*s).body, &previous);
        (*s).loading = false;
        (*s).active = old_id;
        return Err("RichEdit が本文を読み込めません。元の本文を保持しています".into());
    }
    uniform_body_format(s);
    (*s).active = id;
    (*s).dirty = false;
    (*s).loading = false;
    (*s).error = None;
    (*s).restore_position = false;
    SendMessageW((*s).body, 0xcf, usize::from((*s).active.is_none()), 0); // EM_SETREADONLY
    SendMessageW((*s).body, 0xcd, 0, 0); // EM_EMPTYUNDOBUFFER
    SendMessageW((*s).body, 0xb1, 0, 0);
    SendMessageW(
        (*s).body,
        EM_SETSCROLLPOS,
        0,
        &Point::default() as *const _ as isize,
    );
    update_caption(s);
    update_thumb(s, false);
    Ok(())
}
struct LiteralInput {
    units: Vec<u16>,
    position: usize,
}
unsafe extern "system" fn literal_input(
    cookie: usize,
    buffer: *mut u8,
    requested: i32,
    copied: *mut i32,
) -> u32 {
    if cookie == 0 || buffer.is_null() || copied.is_null() || requested < 0 {
        return 1;
    }
    let input = &mut *(cookie as *mut LiteralInput);
    let bytes = input.units.len() * 2;
    let count = bytes
        .saturating_sub(input.position)
        .min(requested as usize & !1);
    ptr::copy_nonoverlapping(
        (input.units.as_ptr() as *const u8).add(input.position),
        buffer,
        count,
    );
    input.position += count;
    *copied = count as i32;
    0
}
unsafe fn set_literal_text(h: Hwnd, value: &str) -> bool {
    // The explicit SF_TEXT stream format prevents RichEdit's RTF prefix
    // detection (observed for WM_SETTEXT and even EM_REPLACESEL). UTF-16LE
    // input has no NUL terminator; the callback ends by returning zero bytes.
    let mut input = LiteralInput {
        units: control_text(value).encode_utf16().collect(),
        position: 0,
    };
    let mut stream = EditStream {
        cookie: &mut input as *mut _ as usize,
        error: 0,
        callback: Some(literal_input),
    };
    SendMessageW(h, 0x449, 0x11, &mut stream as *mut _ as isize); // EM_STREAMIN SF_TEXT|SF_UNICODE
    let stream_error = stream.error;
    stream_error == 0 && input.position == input.units.len() * 2
}
unsafe fn reload_items(s: *mut State, preferred: Option<&str>) {
    (*s).items = if (*s).picker == Some(true) {
        (*s).view.archived.clone()
    } else {
        (*s).view.normal.clone()
    };
    if let Some(index) = preferred.and_then(|id| (*s).items.iter().position(|m| m.id == id)) {
        (*s).selected = index
    } else {
        (*s).selected = (*s).selected.min((*s).items.len().saturating_sub(1));
    }
    ensure_selection(s);
    layout(s);
    InvalidateRect((*s).overlay, ptr::null(), 0);
}
unsafe fn hide_toast(s: *mut State) {
    (*s).undo_archive = None;
    (*s).toast_deadline = None;
    (*s).toast_hover = false;
    (*s).toast_focus = false;
    KillTimer((*s).window, 2);
    InvalidateRect((*s).overlay, ptr::null(), 0);
}
unsafe fn pause_toast(s: *mut State) {
    if (*s).undo_archive.is_some() {
        (*s).toast_deadline = if (*s).toast_hover || (*s).toast_focus {
            None
        } else {
            Some(Instant::now() + Duration::from_secs(8))
        };
    }
}
unsafe fn open_picker(s: *mut State, archived: bool) {
    if (*s).renaming {
        cancel_rename(s)
    }
    if (*s).picker.is_none() {
        SendMessageW(
            (*s).body,
            EM_EXGETSEL,
            0,
            &mut (*s).selection as *mut _ as isize,
        );
        SendMessageW(
            (*s).body,
            EM_GETSCROLLPOS,
            0,
            &mut (*s).scroll as *mut _ as isize,
        );
        (*s).restore_position = true;
    }
    (*s).picker = Some(archived);
    (*s).selected = 0;
    (*s).first = 0;
    hide_toast(s);
    let active = (*s).active.clone();
    reload_items(s, active.as_deref());
    ShowWindow((*s).overlay, 5);
    ShowWindow((*s).thumb, 0);
    SetFocus((*s).overlay);
    snapshot(s);
}
unsafe fn close_picker(s: *mut State) {
    cancel_rename(s);
    (*s).picker = None;
    hide_toast(s);
    ShowWindow((*s).overlay, 0);
    if (*s).restore_position {
        SendMessageW(
            (*s).body,
            EM_EXSETSEL,
            0,
            &(*s).selection as *const _ as isize,
        );
        SendMessageW(
            (*s).body,
            EM_SETSCROLLPOS,
            0,
            &(*s).scroll as *const _ as isize,
        );
    }
    SetFocus((*s).body);
    snapshot(s);
}
unsafe fn switch_memo(s: *mut State, id: &str, restoring: bool) {
    if !restoring && (*s).active.as_deref() == Some(id) {
        close_picker(s);
        return;
    }
    request_action(
        s,
        PendingAction::Storage(Action::Switch {
            id: id.into(),
            restoring,
        }),
    );
}
unsafe fn create_memo(s: *mut State) {
    request_action(s, PendingAction::Storage(Action::Create));
}
unsafe fn select_memo(s: *mut State) {
    if let Some(item) = (&(*s).items).get((*s).selected).cloned() {
        switch_memo(s, &item.id, (*s).picker == Some(true));
    }
}
unsafe fn archive_selected(s: *mut State) {
    if (*s).picker != Some(false) || (*s).renaming {
        return;
    }
    if let Some(item) = (&(*s).items).get((*s).selected).cloned() {
        request_action(s, PendingAction::Storage(Action::Archive { id: item.id }));
    }
}
unsafe fn undo_archive(s: *mut State) {
    if let Some(id) = (*s).undo_archive.clone() {
        request_action(s, PendingAction::Storage(Action::Restore { id }));
    }
}
unsafe fn begin_rename(s: *mut State) {
    if (*s).picker != Some(false) || (*s).renaming {
        return;
    }
    let Some(item) = (&(*s).items).get((*s).selected).cloned() else {
        return;
    };
    (*s).renaming = true;
    (*s).rename_error = None;
    SetWindowTextW((*s).title_edit, wide(&item.title).as_ptr());
    layout(s);
    ShowWindow((*s).title_edit, 5);
    SetFocus((*s).title_edit);
    SendMessageW((*s).title_edit, 0xb1, 0, -1);
    snapshot(s);
}
unsafe fn cancel_rename(s: *mut State) {
    if (*s).renaming {
        (*s).renaming = false;
        (*s).rename_error = None;
        ShowWindow((*s).title_edit, 0);
        InvalidateRect((*s).overlay, ptr::null(), 0);
    }
}
unsafe fn confirm_rename(s: *mut State) {
    if (*s).composing {
        return;
    }
    let Some(item) = (&(*s).items).get((*s).selected).cloned() else {
        return;
    };
    let title = match read_text((*s).title_edit) {
        Ok(t) => t.trim().to_owned(),
        Err(e) => {
            (*s).rename_error = Some(e);
            InvalidateRect((*s).overlay, ptr::null(), 0);
            return;
        }
    };
    if title.is_empty() || title.chars().count() > 200 || title.chars().any(|c| c.is_control()) {
        (*s).rename_error = Some("タイトルを1〜200文字で入力してください".into());
        InvalidateRect((*s).overlay, ptr::null(), 0);
    } else {
        request_action(
            s,
            PendingAction::Storage(Action::Rename { id: item.id, title }),
        );
    }
    snapshot(s);
}
unsafe fn shortcut(s: *mut State, key: usize, mods: usize) -> bool {
    if (*s).composing || key == 0xe5 {
        return false;
    }
    if busy(s) {
        return (*s).picker.is_some() || (mods & 1 != 0 && [0x4e, 0x50, 0x4d].contains(&key));
    }
    let ctrl = mods & 1 != 0;
    let shift = mods & 2 != 0;
    let alt = mods & 4 != 0;
    // Keep system RichEdit's rich-format hotkeys from altering an otherwise plain
    // document. Normal editing, undo, paste, redo, and IME stay in the control.
    if ctrl
        && !alt
        && ([
            0x42, 0x49, 0x55, 0x45, 0x4a, 0x4c, 0x52, 0x31, 0x32, 0x35, 0x20, 0xbb,
        ]
        .contains(&key)
            || (shift && [0x41, 0xbc, 0xbe].contains(&key)))
    {
        return true;
    }
    if (*s).picker.is_some()
        && (key == 0x1b || (ctrl && !alt && key == 0x50 && shift == (*s).picker.unwrap()))
    {
        close_picker(s);
        return true;
    }
    if (*s).renaming {
        if ctrl && !alt && key == 0x50 {
            cancel_rename(s);
            open_picker(s, shift);
            return true;
        }
        if key == 0x0d {
            confirm_rename(s);
            return true;
        }
        if key == 9 {
            SetFocus((*s).title_edit);
            return true;
        }
        if ctrl && !alt && [0x4e, 0x4d].contains(&key) {
            return true;
        }
        return false;
    }
    if ctrl && !alt && key == 0x50 {
        open_picker(s, shift);
        return true;
    }
    if ctrl && !shift && !alt && key == 0x4e {
        create_memo(s);
        return true;
    }
    if (*s).picker.is_some() {
        if key == 9 && (*s).undo_archive.is_some() {
            (*s).toast_focus = !(*s).toast_focus;
            pause_toast(s);
            InvalidateRect((*s).overlay, ptr::null(), 0);
            snapshot(s);
            return true;
        }
        if (*s).toast_focus && (key == 0x0d || key == 0x20) {
            undo_archive(s);
            return true;
        }
        if ctrl && !shift && !alt && key == 0x5a {
            undo_archive(s);
            return true;
        }
        if ctrl && !shift && !alt && key == 0x4d {
            begin_rename(s);
            return true;
        }
        match key {
            0x26 => {
                (*s).toast_focus = false;
                pause_toast(s);
                (*s).selected = (*s).selected.saturating_sub(1);
                ensure_selection(s);
                layout(s);
                InvalidateRect((*s).overlay, ptr::null(), 0);
                snapshot(s);
                true
            }
            0x28 => {
                (*s).toast_focus = false;
                pause_toast(s);
                (*s).selected = ((*s).selected + 1).min((*s).items.len().saturating_sub(1));
                ensure_selection(s);
                layout(s);
                InvalidateRect((*s).overlay, ptr::null(), 0);
                snapshot(s);
                true
            }
            0x0d => {
                select_memo(s);
                true
            }
            0x2e if !ctrl && !alt && !shift => {
                if !(*s).toast_focus {
                    archive_selected(s);
                }
                true
            }
            _ => false,
        }
    } else if ctrl && !alt && [0x4d, 0x51, 0x54, 0x4b].contains(&key) {
        true
    } else if (ctrl && !alt && key == 0x56) || (!ctrl && !alt && shift && key == 0x2d) {
        SendMessageW((*s).body, 0x302, 0, 0);
        true
    } else if ctrl && !alt && !shift && key == 0x41 {
        SendMessageW((*s).body, 0xb1, 0, -1);
        true
    } else {
        false
    }
}
unsafe fn visible_rows(s: *mut State) -> usize {
    (((*s).picker_rect.bottom - (*s).picker_rect.top - px(s, 68)).max(px(s, 48)) / px(s, 48)).max(1)
        as usize
}
unsafe fn ensure_selection(s: *mut State) {
    let rows = visible_rows(s);
    (*s).first = (*s).first.min((*s).items.len().saturating_sub(rows));
    if (*s).selected < (*s).first {
        (*s).first = (*s).selected
    }
    if (*s).selected >= (*s).first + rows {
        (*s).first = (*s).selected + 1 - rows
    }
}
unsafe fn layout(s: *mut State) {
    if (*s).window.is_null() || (*s).body.is_null() {
        return;
    }
    let mut r = Rect::default();
    GetClientRect((*s).window, &mut r);
    let w = r.right;
    let h = r.bottom;
    let shell = w.min(px(s, 1100));
    let shell_x = (w - shell) / 2;
    MoveWindow((*s).body, shell_x, 0, shell, h, 1);
    let narrow = w <= px(s, 600);
    let side = px(s, if narrow { 24 } else { 48 });
    let top = px(s, if narrow { 36 } else { 60 });
    let bottom = px(s, 48);
    let rect = Rect {
        left: side,
        top,
        right: (shell - side).max(side + 20),
        bottom: (h - bottom).max(top + 20),
    };
    SendMessageW((*s).body, 0xb3, 0, &rect as *const _ as isize); // EM_SETRECTNP
    MoveWindow((*s).overlay, 0, 0, w, h, 1);
    let pw = px(s, 560).min((w - px(s, 48)).max(px(s, 120)));
    let max_height = (h - px(s, 180)).max(px(s, 116));
    let rows = (*s).items.len().max(1).min(1000) as i32;
    let ph = (px(s, 68) + px(s, 48) * rows).min(max_height);
    let x = (w - pw) / 2;
    let y = ((h - ph) / 2).max(px(s, 16));
    (*s).picker_rect = Rect {
        left: x,
        top: y,
        right: x + pw,
        bottom: y + ph,
    };
    (*s).row_height = px(s, 48);
    (*s).toast_rect = Rect {
        left: x,
        top: y + ph + px(s, 12),
        right: x + pw,
        bottom: y + ph + px(s, 76),
    };
    ensure_selection(s);
    if (*s).renaming {
        let row = (*s).selected.saturating_sub((*s).first) as i32;
        MoveWindow(
            (*s).title_edit,
            x + px(s, 32),
            y + px(s, 52) + row * px(s, 48) + px(s, 10),
            pw - px(s, 64),
            px(s, 28),
            1,
        );
    }
    update_thumb(s, false);
}
unsafe fn scroll_geometry(s: *mut State) -> (i32, i32, i32, i32) {
    let mut pos = Point::default();
    SendMessageW((*s).body, EM_GETSCROLLPOS, 0, &mut pos as *mut _ as isize);
    let mut info = ScrollInfo {
        size: mem::size_of::<ScrollInfo>() as u32,
        mask: 0x17,
        min: 0,
        max: 0,
        page: 0,
        pos: 0,
        track: 0,
    };
    GetScrollInfo((*s).body, 1, &mut info);
    let max = (info.max - info.page as i32 + 1).max(0);
    let mut r = Rect::default();
    GetClientRect((*s).body, &mut r);
    let track = (r.bottom - px(s, 8)).max(1);
    let total = (info.max + 1).max(1);
    let th = ((track as i64 * info.page as i64 / total as i64) as i32)
        .max(px(s, 44))
        .min(track);
    (pos.y, max, track, th)
}
unsafe fn update_thumb(s: *mut State, reveal: bool) {
    if (*s).body.is_null() || (*s).thumb.is_null() {
        return;
    }
    let (pos, max, track, th) = scroll_geometry(s);
    let mut r = Rect::default();
    GetClientRect((*s).window, &mut r);
    let shell = r.right.min(px(s, 1100));
    let shell_x = (r.right - shell) / 2;
    let y = px(s, 4)
        + if max > 0 {
            ((track - th) as i64 * pos as i64 / max as i64) as i32
        } else {
            0
        };
    let x = shell_x + shell - px(s, 24);
    (*s).thumb_rect = Rect {
        left: x,
        top: y,
        right: x + px(s, 20),
        bottom: y + th,
    };
    MoveWindow((*s).thumb, x, y, px(s, 20), th, 1);
    if reveal && max > 0 && (*s).picker.is_none() {
        (*s).thumb_visible = true;
        ShowWindow((*s).thumb, 5);
        SetTimer((*s).window, 3, 1500, ptr::null_mut());
    }
    if max <= 0 || (*s).picker.is_some() {
        (*s).thumb_visible = false;
        ShowWindow((*s).thumb, 0);
    }
}
unsafe fn text(dc: Handle, r: Rect, value: &str, color: u32, font: Handle, flags: u32) {
    SetTextColor(dc, color);
    SetBkMode(dc, 1);
    let old = SelectObject(dc, font);
    let mut r = r;
    let s = wide(value);
    DrawTextW(dc, s.as_ptr(), (s.len() - 1) as i32, &mut r, flags);
    SelectObject(dc, old);
}
unsafe fn rounded(dc: Handle, r: Rect, color: u32, radius: i32) {
    let b = CreateSolidBrush(color);
    let old = SelectObject(dc, b);
    let pen = SelectObject(dc, GetStockObject(8));
    RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(dc, pen);
    SelectObject(dc, old);
    DeleteObject(b);
}
unsafe fn draw_picker(s: *mut State, h: Hwnd, dc: Handle) {
    let mut full = Rect::default();
    GetClientRect(h, &mut full);
    let bg = CreateSolidBrush(0x161616);
    FillRect(dc, &full, bg);
    DeleteObject(bg);
    let r = (*s).picker_rect;
    rounded(dc, r, PANEL, px(s, 16));
    if (*s).picker == Some(true) {
        text(
            dc,
            Rect {
                left: r.left + px(s, 32),
                top: r.top + px(s, 15),
                right: r.right - px(s, 32),
                bottom: r.top + px(s, 36),
            },
            "アーカイブ",
            0xaaaaaa,
            (*s).small_font,
            0x24,
        );
    }
    let end = ((*s).first + visible_rows(s)).min((*s).items.len());
    for index in (*s).first..end {
        let y = r.top + px(s, 52) + (index - (*s).first) as i32 * px(s, 48);
        let row = Rect {
            left: r.left + px(s, 16),
            top: y,
            right: r.right - px(s, 16),
            bottom: y + px(s, 48),
        };
        if index == (*s).selected {
            rounded(dc, row, SELECTED, px(s, 8));
        }
        if !((*s).renaming && index == (*s).selected) {
            text(
                dc,
                Rect {
                    left: row.left + px(s, 16),
                    top: y,
                    right: row.right - px(s, 16),
                    bottom: row.bottom,
                },
                &(&(*s).items)[index].title,
                FG,
                (*s).font,
                0x24 | 0x8000,
            );
        }
    }
    if let Some(e) = (*s).rename_error.as_deref() {
        text(
            dc,
            Rect {
                left: r.left + px(s, 32),
                top: r.bottom + px(s, 8),
                right: r.right - px(s, 32),
                bottom: r.bottom + px(s, 60),
            },
            e,
            0xa5b8df,
            (*s).small_font,
            0x10,
        );
    }
    if (*s).undo_archive.is_some() {
        let toast = (*s).toast_rect;
        rounded(dc, toast, 0x303030, px(s, 8));
        text(
            dc,
            Rect {
                left: toast.left + px(s, 20),
                top: toast.top,
                right: toast.right - px(s, 112),
                bottom: toast.bottom,
            },
            "アーカイブしました",
            FG,
            (*s).small_font,
            0x24,
        );
        let button = Rect {
            left: toast.right - px(s, 108),
            top: toast.top + px(s, 12),
            right: toast.right - px(s, 14),
            bottom: toast.bottom - px(s, 12),
        };
        rounded(
            dc,
            button,
            if (*s).toast_focus { 0x686868 } else { 0x4b4b4b },
            px(s, 8),
        );
        text(dc, button, "取り消す", FG, (*s).small_font, 0x25);
    }
    if let Some(e) = (*s).error.as_deref() {
        text(
            dc,
            Rect {
                left: px(s, 18),
                top: px(s, 12),
                right: full.right - px(s, 18),
                bottom: px(s, 52),
            },
            e,
            0xb0b0b0,
            (*s).small_font,
            0x2 | 0x8000,
        );
    }
    // WM_PRINTCLIENT renders to another process's DIB. Finish this thread's
    // batched text output before the caller reads the shared pixels.
    GdiFlush();
}
unsafe fn paint_picker(s: *mut State, h: Hwnd) {
    let mut p: Paint = mem::zeroed();
    let dc = BeginPaint(h, &mut p);
    draw_picker(s, h, dc);
    EndPaint(h, &p);
}
unsafe extern "system" fn overlay_proc(h: Hwnd, m: u32, w: usize, l: isize) -> isize {
    let s = STATE;
    if s.is_null() {
        return DefWindowProcW(h, m, w, l);
    }
    if busy(s) && [0x200, 0x201, 0x20a].contains(&m) {
        return 0;
    }
    match m {
        0xf => {
            paint_picker(s, h);
            0
        }
        0x318 => {
            draw_picker(s, h, w as Handle);
            0
        }
        0x14 => 1,
        0x200 => {
            let x = l as i16 as i32;
            let y = (l >> 16) as i16 as i32;
            let toast = (*s).toast_rect;
            let hovered = (*s).undo_archive.is_some()
                && x >= toast.left
                && x < toast.right
                && y >= toast.top
                && y < toast.bottom;
            if hovered != (*s).toast_hover {
                (*s).toast_hover = hovered;
                pause_toast(s);
            }
            let mut track = TrackMouse {
                size: mem::size_of::<TrackMouse>() as u32,
                flags: 2,
                hwnd: h,
                hover: 0,
            };
            TrackMouseEvent(&mut track);
            if !(*s).renaming {
                let r = (*s).picker_rect;
                if x >= r.left + px(s, 16)
                    && x < r.right - px(s, 16)
                    && y >= r.top + px(s, 52)
                    && y < r.bottom - px(s, 16)
                {
                    let i = (*s).first + ((y - r.top - px(s, 52)) / px(s, 48)) as usize;
                    if i < (*s).items.len() && i != (*s).selected {
                        (*s).selected = i;
                        (*s).toast_focus = false;
                        pause_toast(s);
                        InvalidateRect(h, ptr::null(), 0);
                        snapshot(s);
                    }
                }
            }
            0
        }
        0x2a3 => {
            (*s).toast_hover = false;
            pause_toast(s);
            0
        }
        0x201 => {
            let x = l as i16 as i32;
            let y = (l >> 16) as i16 as i32;
            let r = (*s).picker_rect;
            let toast = (*s).toast_rect;
            if (*s).undo_archive.is_some()
                && x >= toast.right - px(s, 108)
                && x < toast.right - px(s, 14)
                && y >= toast.top + px(s, 12)
                && y < toast.bottom - px(s, 12)
            {
                undo_archive(s);
                return 0;
            }
            if x < r.left || x >= r.right || y < r.top || y >= r.bottom {
                return 0;
            }
            if !(*s).renaming && y >= r.top + px(s, 52) && y < r.bottom - px(s, 16) {
                let i = (*s).first + ((y - r.top - px(s, 52)) / px(s, 48)) as usize;
                if i < (*s).items.len() {
                    (*s).selected = i;
                    select_memo(s);
                }
            }
            0
        }
        0x20a => {
            if !(*s).renaming {
                let delta = (w >> 16) as i16 as i32;
                let max = (*s).items.len().saturating_sub(visible_rows(s));
                if delta > 0 {
                    (*s).first = (*s).first.saturating_sub(3)
                } else {
                    (*s).first = ((*s).first + 3).min(max)
                }
                InvalidateRect(h, ptr::null(), 0);
            }
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}
unsafe extern "system" fn rich_proc(h: Hwnd, m: u32, w: usize, l: isize) -> isize {
    let s = STATE;
    if s.is_null() {
        return DefWindowProcW(h, m, w, l);
    }
    let original = if h == (*s).body {
        (*s).body_proc
    } else {
        (*s).title_proc
    };
    if h == (*s).body && (m == 0x302 || (m == WM_CHAR && w == 0x16)) {
        return CallWindowProcW(original, h, 0x440, 13, 0);
    } // Plain Unicode paste only.
    if m == 0x10d {
        (*s).composing = true;
        snapshot(s)
    }
    let result = CallWindowProcW(original, h, m, w, l);
    if m == 0x10e {
        (*s).composing = false;
        if h == (*s).body {
            save(s);
        }
        snapshot(s)
    }
    if h == (*s).body && [0x20a, 0x115, 0x100, 0x102].contains(&m) {
        update_thumb(s, true);
    }
    result
}
unsafe extern "system" fn thumb_proc(h: Hwnd, m: u32, w: usize, l: isize) -> isize {
    let s = STATE;
    if s.is_null() {
        return DefWindowProcW(h, m, w, l);
    }
    match m {
        0xf => {
            let mut p: Paint = mem::zeroed();
            let dc = BeginPaint(h, &mut p);
            let mut r = Rect::default();
            GetClientRect(h, &mut r);
            rounded(
                dc,
                r,
                if (*s).thumb_hover { 0x999999 } else { 0x777777 },
                px(s, 4),
            );
            EndPaint(h, &p);
            0
        }
        0x318 => {
            let mut r = Rect::default();
            GetClientRect(h, &mut r);
            rounded(w as Handle, r, 0x777777, px(s, 4));
            GdiFlush();
            0
        }
        0x14 => 1,
        0x201 => {
            let (pos, _, _, _) = scroll_geometry(s);
            (*s).thumb_drag = Some(((l >> 16) as i16 as i32 + (*s).thumb_rect.top, pos));
            SetCapture(h);
            0
        }
        0x200 => {
            (*s).thumb_hover = true;
            let mut track = TrackMouse {
                size: mem::size_of::<TrackMouse>() as u32,
                flags: 2,
                hwnd: h,
                hover: 0,
            };
            TrackMouseEvent(&mut track);
            if let Some((start, pos)) = (*s).thumb_drag {
                let (_, max, track, th) = scroll_geometry(s);
                let y = (l >> 16) as i16 as i32 + (*s).thumb_rect.top;
                let new = (pos
                    + ((y - start) as i64 * max as i64 / (track - th).max(1) as i64) as i32)
                    .clamp(0, max);
                SendMessageW(
                    (*s).body,
                    EM_SETSCROLLPOS,
                    0,
                    &Point { x: 0, y: new } as *const _ as isize,
                );
                update_thumb(s, true);
            }
            InvalidateRect(h, ptr::null(), 0);
            0
        }
        0x202 => {
            (*s).thumb_drag = None;
            ReleaseCapture();
            SetTimer((*s).window, 3, 1500, ptr::null_mut());
            0
        }
        0x2a3 => {
            (*s).thumb_hover = false;
            SetTimer((*s).window, 3, 1500, ptr::null_mut());
            InvalidateRect(h, ptr::null(), 0);
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}
unsafe fn snapshot(s: *mut State) {
    let Some(path) = (*s).report.clone() else {
        return;
    };
    if (*s).body.is_null() {
        return;
    }
    let value = read_text((*s).body).unwrap_or_default();
    let title_value = read_text((*s).title_edit).unwrap_or_default();
    let items = (*s)
        .items
        .iter()
        .map(|m| {
            format!(
                "{{\"id\":{},\"title\":{},\"created\":{},\"archived\":{}}}",
                json(&m.id),
                json(&m.title),
                m.created,
                m.archived
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let catalog = (*s)
        .view
        .normal
        .iter()
        .map(|m| format!("{{\"id\":{},\"title\":{}}}", json(&m.id), json(&m.title)))
        .collect::<Vec<_>>()
        .join(",");
    let archives = (*s)
        .view
        .archived
        .iter()
        .map(|m| format!("{{\"id\":{},\"title\":{}}}", json(&m.id), json(&m.title)))
        .collect::<Vec<_>>()
        .join(",");
    let mut cf: CharFormat = mem::zeroed();
    cf.size = mem::size_of::<CharFormat>() as u32;
    SendMessageW((*s).body, EM_GETCHARFORMAT, 0, &mut cf as *mut _ as isize);
    let mut pf: ParaFormat = mem::zeroed();
    pf.size = mem::size_of::<ParaFormat>() as u32;
    SendMessageW((*s).body, EM_GETPARAFORMAT, 0, &mut pf as *mut _ as isize);
    let face =
        String::from_utf16_lossy(&cf.face[..cf.face.iter().position(|v| *v == 0).unwrap_or(32)]);
    let mut rect = Rect::default();
    SendMessageW((*s).body, 0xb2, 0, &mut rect as *mut _ as isize);
    let mut data=format!("{{\"window\":{},\"editor\":{},\"overlay\":{},\"titleEditor\":{},\"thumb\":{},\"ready\":{},\"hidden\":{},\"visible\":{},\"active\":{},\"text\":{},\"dirty\":{},\"error\":{},\"picker\":{},\"selected\":{},\"first\":{},\"items\":[{}],\"catalog\":[{}],\"archives\":[{}],\"renaming\":{},\"titleText\":{},\"renameError\":{},\"composing\":{},\"undoArchive\":{},\"saveCount\":{},\"readyMs\":{:.3},\"scale\":{},\"charFormatSize\":{},\"paraFormatSize\":{},\"font\":{},\"fontTwips\":{},\"spacingTwips\":{},\"lineSpacingTwips\":{},\"lineRule\":{},\"formatRect\":[{},{},{},{}],\"pickerRect\":[{},{},{},{}]}}",
 (*s).window as usize,(*s).body as usize,(*s).overlay as usize,(*s).title_edit as usize,(*s).thumb as usize,(*s).ready,(*s).hidden,IsWindowVisible((*s).window)!=0,(*s).active.as_deref().map(json).unwrap_or("null".into()),json(&value),(*s).dirty,(*s).error.as_deref().map(json).unwrap_or("null".into()),(*s).picker.map(|a|if a{"\"archived\""}else{"\"normal\""}).unwrap_or("null"),(*s).selected,(*s).first,items,catalog,archives,(*s).renaming,json(&title_value),(*s).rename_error.as_deref().map(json).unwrap_or("null".into()),(*s).composing,(*s).undo_archive.as_deref().map(json).unwrap_or("null".into()),(*s).save_count,(*s).ready_ms,(*s).scale,cf.size,pf.size,json(&face),cf.height,cf.spacing,pf.line_spacing,pf.line_rule,rect.left,rect.top,rect.right,rect.bottom,(*s).picker_rect.left,(*s).picker_rect.top,(*s).picker_rect.right,(*s).picker_rect.bottom);
    data.pop();
    data.push_str(&format!(
        ",\"saving\":{},\"busy\":{},\"generation\":{},\"savedGeneration\":{},\"lastSaveMs\":{}",
        (*s).saving,
        busy(s),
        (*s).generation,
        (*s).saved_generation,
        (*s).last_save_ms
    ));
    data.push_str(&format!(",\"toastFocused\":{},\"toastHovered\":{},\"textMode\":{},\"recovery\":{},\"paragraphApplied\":{}}}",(*s).toast_focus,(*s).toast_hover,SendMessageW((*s).body,0x45a,0,0),(*s).recovery.as_ref().map(|p|json(&p.to_string_lossy())).unwrap_or("null".into()),(*s).paragraph_applied));
    // Reports are optional test instrumentation, never a second copy of drafts in
    // normal use. Write replacement so readers do not observe half JSON.
    let pending = path.with_extension("report.pending");
    if fs::write(&pending, data).is_ok() {
        let _ = fs::rename(&pending, &path);
    }
}
unsafe extern "system" fn window_proc(h: Hwnd, m: u32, w: usize, l: isize) -> isize {
    let s = STATE;
    if s.is_null() {
        return DefWindowProcW(h, m, w, l);
    }
    match m {
        5 => {
            layout(s);
            snapshot(s);
            0
        }
        7 => {
            if (*s).ready && !(*s).hidden {
                SetFocus(if (*s).renaming {
                    (*s).title_edit
                } else if (*s).picker.is_some() {
                    (*s).overlay
                } else {
                    (*s).body
                });
            }
            0
        }
        0x111 if (w >> 16) & 0xffff == 0x300 => {
            if l as Hwnd == (*s).body && !(*s).loading && (*s).ready {
                (*s).dirty = true;
                (*s).generation += 1;
                if !(*s).composing {
                    save(s);
                }
                update_thumb(s, false);
            }
            snapshot(s);
            0
        }
        0x113 => {
            match w {
                1 => {
                    if (*s).dirty && !(*s).composing {
                        save(s);
                    }
                }
                2 => {
                    if (*s).toast_deadline.is_some_and(|t| Instant::now() >= t) {
                        hide_toast(s);
                        snapshot(s);
                    }
                }
                3 => {
                    if !(*s).thumb_hover && (*s).thumb_drag.is_none() {
                        (*s).thumb_visible = false;
                        ShowWindow((*s).thumb, 0);
                        KillTimer(h, 3);
                    }
                }
                5 => {
                    poll_storage(s);
                    if !(*s).inflight && !(*s).dirty {
                        continue_action(s);
                    }
                }
                4 => {
                    PostMessageW(h, 0x10, 0, 0);
                }
                _ => {}
            }
            0
        }
        0xf => {
            let mut p: Paint = mem::zeroed();
            let dc = BeginPaint(h, &mut p);
            FillRect(dc, &p.rect, (*s).brush);
            if let Some(e) = (*s).error.as_deref() {
                let mut r = Rect::default();
                GetClientRect(h, &mut r);
                text(
                    dc,
                    Rect {
                        left: px(s, 18),
                        top: px(s, 12),
                        right: r.right - px(s, 18),
                        bottom: px(s, 48),
                    },
                    e,
                    0xb0b0b0,
                    (*s).small_font,
                    0x2 | 0x8000,
                );
            }
            EndPaint(h, &p);
            0
        }
        0x14 => 1,
        0x318 => {
            let mut r = Rect::default();
            GetClientRect(h, &mut r);
            FillRect(w as Handle, &r, (*s).brush);
            GdiFlush();
            0
        }
        TEST_SHORTCUT if (*s).test_mode => {
            let handled = shortcut(s, w, l as usize);
            snapshot(s);
            isize::from(handled)
        }
        TEST_SNAPSHOT if (*s).test_mode => {
            snapshot(s);
            1
        }
        TEST_CLOSE if (*s).test_mode => {
            PostMessageW(h, 0x10, 0, 0);
            1
        }
        0x10 => {
            request_action(s, PendingAction::Close);
            0
        }
        2 => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}
unsafe fn configure_rich(_s: *mut State, h: Hwnd, body: bool) -> Result<(), String> {
    // The system RichEdit rejects paragraph spacing in TM_PLAINTEXT (verified).
    // Use its richer layout engine for the body, while plain Unicode input and
    // paste, literal .txt loading, and disabled formatting keys keep .txt semantics.
    if SendMessageW(h, 0x459, if body { 2 | 8 | 32 } else { 1 | 8 | 32 }, 0) != 0 {
        return Err("RichEdit の入力モードを設定できません".into());
    } // EM_SETTEXTMODE
    SendMessageW(h, 0x443, 0, BG as isize); // EM_SETBKGNDCOLOR
    SendMessageW(h, 0x435, 0, 0x7ffffffe); // EM_EXLIMITTEXT
    SendMessageW(h, 0x445, 0, 1); // ENM_CHANGE
                                  // RichEdit's script association chooses the Japanese face without rewriting
                                  // user text into individual formatted runs or replacing its IME handling.
    SendMessageW(h, 0x478, 0, 2); // EM_SETLANGOPTIONS IMF_AUTOFONT, no keyboard switching.
    let mut cf: CharFormat = mem::zeroed();
    cf.size = mem::size_of::<CharFormat>() as u32;
    cf.mask = 0xec200000; // SIZE FACE COLOR SPACING BACKCOLOR CHARSET
    cf.height = if body { 300 } else { 240 };
    cf.spacing = if body { 3 } else { 0 };
    cf.color = FG;
    cf.bg = if body { BG } else { SELECTED };
    cf.charset = 1;
    let face = wide("Intel One Mono");
    cf.face[..face.len()].copy_from_slice(&face);
    SendMessageW(h, EM_SETCHARFORMAT, 0, &cf as *const _ as isize);
    let mut japanese: CharFormat = mem::zeroed();
    japanese.size = mem::size_of::<CharFormat>() as u32;
    japanese.mask = 0xaa000000;
    japanese.height = cf.height;
    japanese.charset = 128;
    japanese.locale = 0x411;
    let fallback = wide("BIZ UDGothic");
    japanese.face[..fallback.len()].copy_from_slice(&fallback);
    SendMessageW(
        h,
        EM_SETCHARFORMAT,
        0x10 | 0x100,
        &japanese as *const _ as isize,
    ); // SCF_ASSOCIATEFONT | SCF_CHARREPFROMLCID
    if body {
        let mut pf: ParaFormat = mem::zeroed();
        pf.size = mem::size_of::<ParaFormat>() as u32;
        pf.mask = 0x100 | 0x40 | 0x80 | 0x10;
        pf.line_rule = 4;
        pf.line_spacing = 540;
        pf.tabs = 1;
        pf.tabstops[0] = 720;
        SendMessageW(h, EM_SETPARAFORMAT, 0, &pf as *const _ as isize);
    }
    SendMessageW(h, 0x460, 1, 0); // EM_SHOWSCROLLBAR SB_VERT false
    SendMessageW(h, 0x460, 0, 0); // SB_HORZ false
    if !body {
        SendMessageW(h, 0x435, 0, 400);
    }
    Ok(())
}
unsafe fn uniform_body_format(s: *mut State) {
    // WM_SETTEXT resets paragraph formatting. Apply the single document style
    // after a load, before the new document's undo history begins. Editing does
    // not reapply this or touch the user's selection/undo buffer.
    let h = (*s).body;
    let mut selection = CharRange::default();
    let mut scroll = Point::default();
    SendMessageW(h, EM_EXGETSEL, 0, &mut selection as *mut _ as isize);
    SendMessageW(h, EM_GETSCROLLPOS, 0, &mut scroll as *mut _ as isize);
    SendMessageW(
        h,
        EM_EXSETSEL,
        0,
        &CharRange { min: 0, max: -1 } as *const _ as isize,
    );
    let mut cf: CharFormat = mem::zeroed();
    cf.size = mem::size_of::<CharFormat>() as u32;
    cf.mask = 0xec200000;
    cf.height = 300;
    cf.spacing = 3;
    cf.color = FG;
    cf.bg = BG;
    cf.charset = 1;
    let face = wide("Intel One Mono");
    cf.face[..face.len()].copy_from_slice(&face);
    SendMessageW(h, EM_SETCHARFORMAT, 4, &cf as *const _ as isize);
    let mut pf: ParaFormat = mem::zeroed();
    pf.size = mem::size_of::<ParaFormat>() as u32;
    pf.mask = 0x100 | 0x40 | 0x80 | 0x10;
    pf.line_rule = 4;
    pf.line_spacing = 540;
    pf.tabs = 1;
    pf.tabstops[0] = 720;
    (*s).paragraph_applied = SendMessageW(h, EM_SETPARAFORMAT, 0, &pf as *const _ as isize) != 0;
    SendMessageW(h, EM_EXSETSEL, 0, &selection as *const _ as isize);
    SendMessageW(h, EM_SETSCROLLPOS, 0, &scroll as *const _ as isize);
}
unsafe fn run() -> Result<(), String> {
    let started = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| -> Result<Option<String>, String> {
        args.iter()
            .position(|v| v == flag)
            .map(|i| {
                args.get(i + 1)
                    .cloned()
                    .ok_or_else(|| format!("{flag} requires a value"))
            })
            .transpose()
    };
    let probe = args.iter().any(|v| v == "--probe");
    let test_mode = args.iter().any(|v| v == "--test-mode") || probe;
    let explicit = value("--data-dir")?;
    if test_mode && explicit.is_none() {
        return Err("Test and probe modes require an isolated --data-dir".into());
    }
    let save_delay = match value("--test-save-delay-ms")? {
        Some(raw) if test_mode => Duration::from_millis(
            raw.parse::<u64>()
                .map_err(|_| "Invalid save delay")?
                .min(5000),
        ),
        Some(_) => return Err("Save delay requires --test-mode and an isolated --data-dir".into()),
        None => Duration::ZERO,
    };
    let mut startup: StartupInfo = mem::zeroed();
    startup.size = mem::size_of::<StartupInfo>() as u32;
    GetStartupInfoW(&mut startup);
    let hidden = probe || (startup.flags & 1 != 0 && startup.show == 0);
    let dir = match explicit {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .and_then(Path::parent)
            .ok_or("Expected executable in build/")?
            .join("data/native-next"),
    };
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let canonical = fs::canonicalize(&dir).map_err(|e| e.to_string())?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    canonical
        .as_os_str()
        .encode_wide()
        .map(|c| if (65..=90).contains(&c) { c + 32 } else { c })
        .collect::<Vec<_>>()
        .hash(&mut hash);
    let unique = hash.finish();
    let class = wide(&format!("MinimalEditorNext-{unique:016x}"));
    let store = match Store::open(dir) {
        Ok(s) => s,
        Err(e) => {
            if !hidden && !test_mode {
                for _ in 0..40 {
                    let existing = FindWindowW(class.as_ptr(), ptr::null());
                    if !existing.is_null() {
                        ShowWindowAsync(existing, 9);
                        SetForegroundWindow(existing);
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
            return Err(e);
        }
    };
    SetProcessDpiAwarenessContext(-4isize as Handle);
    let instance = GetModuleHandleW(ptr::null());
    if LoadLibraryW(wide("msftedit.dll").as_ptr()).is_null() {
        return Err(err());
    }
    let brush = CreateSolidBrush(BG);
    let overlay_class = wide(&format!("MinimalEditorNextPicker-{unique:016x}"));
    let thumb_class = wide(&format!("MinimalEditorNextThumb-{unique:016x}"));
    for (name, proc) in [
        (
            &class,
            window_proc as unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize,
        ),
        (&overlay_class, overlay_proc),
        (&thumb_class, thumb_proc),
    ] {
        let wc = WndClass {
            style: 3,
            proc: Some(proc),
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon: ptr::null_mut(),
            cursor: LoadCursorW(ptr::null_mut(), 32512usize as *const u16),
            background: brush,
            menu: ptr::null(),
            name: name.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            return Err(err());
        }
    }
    let view = View::read(&store);
    let initial_id = view.active.clone();
    let initial_text = match initial_id.as_deref() {
        Some(id) => store.text(id)?,
        None => String::new(),
    };
    let worker = Worker::start(store, save_delay)?;
    let s = Box::into_raw(Box::new(State {
        worker,
        view,
        inflight: false,
        saving: false,
        action_inflight: false,
        close_after_action: false,
        pending_action: None,
        worker_failed: false,
        generation: 0,
        saved_generation: 0,
        last_save_ms: 0.0,
        window: ptr::null_mut(),
        body: ptr::null_mut(),
        overlay: ptr::null_mut(),
        title_edit: ptr::null_mut(),
        thumb: ptr::null_mut(),
        body_proc: None,
        title_proc: None,
        brush,
        font: ptr::null_mut(),
        small_font: ptr::null_mut(),
        active: None,
        dirty: false,
        loading: false,
        ready: false,
        hidden,
        test_mode,
        report: value("--report-file")?.map(PathBuf::from),
        error: None,
        picker: None,
        items: vec![],
        selected: 0,
        first: 0,
        renaming: false,
        composing: false,
        rename_error: None,
        undo_archive: None,
        toast_deadline: None,
        toast_hover: false,
        toast_focus: false,
        picker_rect: Rect::default(),
        toast_rect: Rect::default(),
        row_height: 48,
        scale: 1.0,
        selection: CharRange::default(),
        scroll: Point::default(),
        restore_position: false,
        thumb_visible: false,
        thumb_hover: false,
        thumb_drag: None,
        thumb_rect: Rect::default(),
        ready_ms: 0.0,
        save_count: 0,
        recovery: None,
        paragraph_applied: false,
    }));
    STATE = s;
    (*s).window = CreateWindowExW(
        0,
        class.as_ptr(),
        wide(TITLE).as_ptr(),
        0x02cf0000,
        0x80000000u32 as i32,
        0x80000000u32 as i32,
        1100,
        800,
        ptr::null_mut(),
        ptr::null_mut(),
        instance,
        ptr::null_mut(),
    );
    if (*s).window.is_null() {
        return Err(err());
    }
    (*s).scale = GetDpiForWindow((*s).window) as f64 / 96.0;
    let dark = 1i32;
    DwmSetWindowAttribute((*s).window, 20, &dark as *const _ as *const c_void, 4);
    (*s).font = CreateFontW(
        -px(s, 16),
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
    (*s).small_font = CreateFontW(
        -px(s, 14),
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
    (*s).body = CreateWindowExW(
        0,
        wide("RICHEDIT50W").as_ptr(),
        wide("").as_ptr(),
        0x40000000 | 0x10000000 | 0x00200000 | 0x00010000 | 4 | 8 | 0x40 | 0x1000,
        0,
        0,
        100,
        100,
        (*s).window,
        1usize as Handle,
        instance,
        ptr::null_mut(),
    );
    if (*s).body.is_null() {
        return Err(err());
    } // ES_NOOLEDRAGDROP=8
    configure_rich(s, (*s).body, true)?;
    (*s).body_proc = mem::transmute(SetWindowLongPtrW(
        (*s).body,
        -4,
        rich_proc as *const () as isize,
    ));
    (*s).thumb = CreateWindowExW(
        0,
        thumb_class.as_ptr(),
        wide("").as_ptr(),
        0x40000000,
        0,
        0,
        20,
        44,
        (*s).window,
        4usize as Handle,
        instance,
        ptr::null_mut(),
    );
    (*s).overlay = CreateWindowExW(
        0,
        overlay_class.as_ptr(),
        wide("メモ").as_ptr(),
        0x40000000 | 0x02000000,
        0,
        0,
        100,
        100,
        (*s).window,
        2usize as Handle,
        instance,
        ptr::null_mut(),
    );
    (*s).title_edit = CreateWindowExW(
        0,
        wide("RICHEDIT50W").as_ptr(),
        wide("").as_ptr(),
        0x40000000 | 0x10000 | 0x80,
        0,
        0,
        100,
        30,
        (*s).overlay,
        3usize as Handle,
        instance,
        ptr::null_mut(),
    );
    if (*s).overlay.is_null() || (*s).title_edit.is_null() || (*s).thumb.is_null() {
        return Err(err());
    }
    configure_rich(s, (*s).title_edit, false)?;
    SendMessageW((*s).title_edit, 0x443, 0, SELECTED as isize);
    (*s).title_proc = mem::transmute(SetWindowLongPtrW(
        (*s).title_edit,
        -4,
        rich_proc as *const () as isize,
    ));
    load_text(s, initial_id, &initial_text)?;
    (*s).ready = true;
    layout(s);
    (*s).ready_ms = started.elapsed().as_secs_f64() * 1000.0;
    SetTimer((*s).window, 1, 2000, ptr::null_mut());
    if !hidden {
        ShowWindow(
            (*s).window,
            if startup.flags & 1 != 0 {
                startup.show as i32
            } else {
                5
            },
        );
        SetFocus((*s).body);
    }
    if let Some(hold) = value("--hold-ms")? {
        SetTimer(
            (*s).window,
            4,
            hold.parse::<u32>().map_err(|_| "Invalid --hold-ms")?.max(1),
            ptr::null_mut(),
        );
    } else if probe {
        SetTimer((*s).window, 4, 1, ptr::null_mut());
    }
    snapshot(s);
    let mut msg: Msg = mem::zeroed();
    loop {
        let status = GetMessageW(&mut msg, ptr::null_mut(), 0, 0);
        if status == 0 {
            break;
        }
        if status < 0 {
            return Err(err());
        }
        if msg.message == WM_KEYDOWN {
            if (*s).picker.is_some()
                && !(*s).renaming
                && msg.wparam == 0x2e
                && msg.lparam & (1 << 30) != 0
            {
                continue;
            }
            let modifiers = usize::from(GetKeyState(0x11) < 0)
                | (usize::from(GetKeyState(0x10) < 0) << 1)
                | (usize::from(GetKeyState(0x12) < 0) << 2);
            if shortcut(s, msg.wparam, modifiers) {
                continue;
            }
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    STATE = ptr::null_mut();
    let owned = Box::from_raw(s);
    DeleteObject(owned.font);
    DeleteObject(owned.small_font);
    DeleteObject(owned.brush);
    drop(owned);
    Ok(())
}
fn main() {
    if let Err(message) = unsafe { run() } {
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|v| v == "--report-file") {
            if let Some(path) = args.get(i + 1) {
                let _ = fs::write(path, format!("{{\"fatal\":{}}}", json(&message)));
            }
        }
        if !args.iter().any(|v| v == "--probe" || v == "--test-mode") {
            unsafe {
                let mut startup: StartupInfo = mem::zeroed();
                GetStartupInfoW(&mut startup);
                if !(startup.flags & 1 != 0 && startup.show == 0) {
                    MessageBoxW(
                        ptr::null_mut(),
                        wide(&message).as_ptr(),
                        wide("minimal-editor · Native").as_ptr(),
                        0x10,
                    );
                }
            }
        }
        std::process::exit(1)
    }
}
