//! A silent startup error dialog. MessageBox can play a system sound, so use
//! a plain dialog with static text and an OK button instead.
use crate::win::*;
use std::ptr;

unsafe extern "system" fn dialog_proc(h: Hwnd, message: u32, w: usize, _l: isize) -> isize {
    match message {
        0x110 => 1, // WM_INITDIALOG: let the dialog manager focus the OK button.
        0x111 if w & 0xffff == 1 || w & 0xffff == 2 => {
            EndDialog(h, 0);
            1
        }
        0x10 => {
            EndDialog(h, 0);
            1
        }
        0x106 if w != 0x20 => 1, // Unassigned Alt characters; keep Alt+Space.
        0x120 => {
            SetWindowLongPtrW(h, 0, 1 << 16); // DWLP_MSGRESULT = MNC_CLOSE.
            1
        }
        _ => 0,
    }
}

struct Template(Vec<u8>);
impl Template {
    fn word(&mut self, value: u16) {
        self.0.extend(value.to_le_bytes());
    }
    fn dword(&mut self, value: u32) {
        self.0.extend(value.to_le_bytes());
    }
    fn text(&mut self, value: &str) {
        for unit in value.encode_utf16().chain(Some(0)) {
            self.word(unit);
        }
    }
    fn align(&mut self) {
        while self.0.len() % 4 != 0 {
            self.0.push(0);
        }
    }
    fn item(&mut self, style: u32, rect: [u16; 4], id: u16, class: u16, text: &str) {
        self.align();
        self.dword(style);
        self.dword(0);
        for value in rect {
            self.word(value);
        }
        self.word(id);
        self.word(0xffff);
        self.word(class);
        self.text(text);
        self.word(0); // No creation data.
    }
}

pub unsafe fn show(message: &str) {
    let mut template = Template(Vec::new());
    // Standard DLGTEMPLATE, in dialog units; no icon or sound-producing controls.
    template.dword(0x80c808c0); // POPUP | CAPTION | SYSMENU | MODALFRAME | CENTER | SETFONT.
    template.dword(0);
    for value in [2, 0, 0, 320, 150, 0, 0] { // Two controls, rect, no menu/default class.
        template.word(value);
    }
    template.text("minimal-editor · Native");
    template.word(9);
    template.text("Segoe UI");
    template.item(0x50000080, [12, 12, 296, 100], 0xffff, 0x82, message); // STATIC / NOPREFIX.
    template.item(0x50010001, [248, 120, 60, 18], 1, 0x80, "OK"); // BUTTON / default.
    template.align();
    // DLGTEMPLATE and each item require DWORD alignment.
    let aligned: Vec<u32> = template.0.chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    DialogBoxIndirectParamW(GetModuleHandleW(ptr::null()), aligned.as_ptr().cast(),
                           ptr::null_mut(), Some(dialog_proc), 0);
}
