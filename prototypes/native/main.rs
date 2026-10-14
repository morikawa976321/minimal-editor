//! Dependency-free Windows EDIT prototype. Build with rustc --edition=2021.
//! --test --data-dir <empty folder> runs hidden controls; --probe loads without edits.
//! --hold-ms N keeps the hidden process alive after its test report for kill tests.
#![windows_subsystem = "windows"]
#![allow(non_snake_case)]

use std::{ffi::c_void, fs, io::{self, Write}, os::windows::ffi::OsStrExt, path::{Path, PathBuf}, ptr, time::Instant};
type Handle = *mut c_void;
type Hwnd = Handle;
#[repr(C)] struct Point { x: i32, y: i32 }
#[repr(C)] struct Rect { left: i32, top: i32, right: i32, bottom: i32 }
#[repr(C)] struct Msg { hwnd: Hwnd, message: u32, wparam: usize, lparam: isize, time: u32, pt: Point, private: u32 }
#[repr(C)] struct WndClass { style: u32, proc: Option<unsafe extern "system" fn(Hwnd,u32,usize,isize)->isize>, cls_extra:i32, wnd_extra:i32, instance:Handle, icon:Handle, cursor:Handle, background:Handle, menu:*const u16, name:*const u16 }
#[link(name="user32")] extern "system" {
    fn RegisterClassW(c:*const WndClass)->u16;
    fn CreateWindowExW(ex:u32, cls:*const u16, title:*const u16, style:u32, x:i32,y:i32,w:i32,h:i32,parent:Hwnd,menu:Handle,instance:Handle,param:Handle)->Hwnd;
    fn DefWindowProcW(h:Hwnd,m:u32,w:usize,l:isize)->isize;
    fn ShowWindow(h:Hwnd,n:i32)->i32;
    fn SetFocus(h:Hwnd)->Hwnd;
    fn GetMessageW(m:*mut Msg,h:Hwnd,min:u32,max:u32)->i32;
    fn TranslateMessage(m:*const Msg)->i32;
    fn DispatchMessageW(m:*const Msg)->isize;
    fn PostQuitMessage(n:i32);
    fn DestroyWindow(h:Hwnd)->i32;
    fn SendMessageW(h:Hwnd,m:u32,w:usize,l:isize)->isize;
    fn SetWindowTextW(h:Hwnd,s:*const u16)->i32;
    fn GetWindowTextLengthW(h:Hwnd)->i32;
    fn GetWindowTextW(h:Hwnd,s:*mut u16,n:i32)->i32;
    fn GetClientRect(h:Hwnd,r:*mut Rect)->i32;
    fn MoveWindow(h:Hwnd,x:i32,y:i32,w:i32,ht:i32,repaint:i32)->i32;
    fn LoadCursorW(instance:Handle,name:*const u16)->Handle;
    fn MessageBoxW(h:Hwnd,text:*const u16,title:*const u16,kind:u32)->i32;
    fn SetProcessDPIAware()->i32;
    fn GetKeyState(key:i32)->i16;
}
#[link(name="kernel32")] extern "system" {
    fn GetModuleHandleW(name:*const u16)->Handle;
    fn CreateFileW(name:*const u16,access:u32,share:u32,security:Handle,creation:u32,flags:u32,template:Handle)->Handle;
    fn CloseHandle(h:Handle)->i32;
    fn ReplaceFileW(target:*const u16,replacement:*const u16,backup:*const u16,flags:u32,exclude:Handle,reserved:Handle)->i32;
    fn MoveFileExW(from:*const u16,to:*const u16,flags:u32)->i32;
}
#[link(name="gdi32")] extern "system" {
    fn CreateSolidBrush(color:u32)->Handle;
    fn SetTextColor(dc:Handle,color:u32)->u32;
    fn SetBkColor(dc:Handle,color:u32)->u32;
    fn CreateFontW(height:i32,width:i32,esc:i32,orient:i32,weight:i32,italic:u32,underline:u32,strike:u32,charset:u32,out:u32,clip:u32,quality:u32,pitch:u32,face:*const u16)->Handle;
    fn DeleteObject(h:Handle)->i32;
}
#[link(name="dwmapi")] extern "system" { fn DwmSetWindowAttribute(h:Hwnd,attribute:u32,value:*const c_void,size:u32)->i32; }

const BG:u32 = 0x001b1a18; // COLORREF is BGR.
const FG:u32 = 0x00e4e1dc;
const TITLE:&str = "Minimal Editor · Native";
fn wide(s:&str)->Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn path_wide(p:&Path)->Vec<u16> { p.as_os_str().encode_wide().chain(Some(0)).collect() }
fn normalized(s:&str)->String { s.replace("\r\n","\n").replace('\r',"\n") }
fn control_text(s:&str)->String { normalized(s).replace('\n',"\r\n") }
fn read_optional(path:&Path)->io::Result<Option<Vec<u8>>> {
    match fs::read(path) { Ok(b)=>Ok(Some(b)), Err(e) if e.kind()==io::ErrorKind::NotFound=>Ok(None), Err(e)=>Err(e) }
}
struct FileLock(Handle);
impl Drop for FileLock { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
fn lock(dir:&Path)->io::Result<FileLock> {
    let p=path_wide(&dir.join("native.lock"));
    let h=unsafe { CreateFileW(p.as_ptr(),0xc0000000,0,ptr::null_mut(),4,0x80,ptr::null_mut()) };
    if h as isize == -1 { Err(io::Error::last_os_error()) } else { Ok(FileLock(h)) }
}
struct State { edit:Hwnd, window:Hwnd, brush:Handle, font:Handle, path:PathBuf, disk:Option<Vec<u8>>, active:bool, dirty:bool, last_error:Option<String>, save_count:u32, hidden:bool }
static mut STATE:*mut State=ptr::null_mut();

/// Save to a flushed same-directory temporary, then replace the old file with a
/// backup in one OS operation. Never delete the original before replacement.
fn atomic_save(path:&Path, bytes:&[u8], old:&Option<Vec<u8>>)->io::Result<()> {
    if &read_optional(path)? != old { return Err(io::Error::other("ファイルが外部で変更されました。上書きを停止しました。")); }
    let tmp=path.with_extension("txt.pending");
    let mut file=fs::OpenOptions::new().write(true).create(true).truncate(true).open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    let target=path_wide(path); let replacement=path_wide(&tmp);
    let result=unsafe {
        if old.is_some() {
            let backup=path_wide(&path.with_extension("txt.bak"));
            ReplaceFileW(target.as_ptr(),replacement.as_ptr(),backup.as_ptr(),0,ptr::null_mut(),ptr::null_mut())
        } else { MoveFileExW(replacement.as_ptr(),target.as_ptr(),8) }
    };
    if result==0 { return Err(io::Error::last_os_error()); }
    Ok(())
}
unsafe fn get_text(edit:Hwnd)->io::Result<String> {
    let len=GetWindowTextLengthW(edit);
    let mut buf=vec![0u16;len as usize+1];
    let got=GetWindowTextW(edit,buf.as_mut_ptr(),buf.len() as i32);
    if got!=len { return Err(io::Error::other("本文を取得できませんでした。保存を停止しました。")); }
    String::from_utf16(&buf[..got as usize]).map(|s| normalized(&s)).map_err(|e|io::Error::new(io::ErrorKind::InvalidData,e))
}
// User32 calls may synchronously reenter window_proc. Keep only raw pointers
// across them: no &State or &mut State remains borrowed while a callback runs.
unsafe fn save(s:*mut State)->bool {
    if !(*s).active || !(*s).dirty { return true; }
    let edit=(*s).edit;
    let outcome=get_text(edit).and_then(|text| {
        let bytes=text.into_bytes();
        if (*s).disk.as_ref()!=Some(&bytes) {
            atomic_save(&(*s).path,&bytes,&(*s).disk)?;
            (*s).disk=Some(bytes); (*s).save_count+=1;
        }
        Ok(())
    });
    match outcome {
        Ok(())=> { (*s).dirty=false; (*s).last_error=None; SetWindowTextW((*s).window,wide(TITLE).as_ptr()); true },
        Err(e)=> { (*s).last_error=Some(e.to_string()); SetWindowTextW((*s).window,wide("未保存 — 閉じずに本文をコピーしてください · Native").as_ptr()); false }
    }
}
unsafe fn layout(s:*mut State) {
    let edit=(*s).edit; let window=(*s).window;
    if edit.is_null() { return; }
    let mut r=Rect{left:0,top:0,right:0,bottom:0}; GetClientRect(window,&mut r);
    let width=(r.right-64).max(80).min(840); let x=((r.right-width)/2).max(8);
    MoveWindow(edit,x,36,width,(r.bottom-72).max(40),1);
}
// Standard EDIT's built-in menu has Select All, but its legacy keyboard
// interface does not guarantee Ctrl+A. Route only this exact key before
// TranslateMessage; IME messages and VK_PROCESSKEY retain the default path.
unsafe fn editor_shortcut(edit:Hwnd,msg:&Msg,ctrl:bool,alt:bool)->bool {
    if msg.hwnd==edit && msg.message==0x0100 && msg.wparam==0x41 && ctrl && !alt {
        SendMessageW(edit,0x00b1,0,-1); // EM_SETSEL, entire text.
        return true;
    }
    false
}
unsafe extern "system" fn window_proc(h:Hwnd,m:u32,w:usize,l:isize)->isize {
    if STATE.is_null() { return DefWindowProcW(h,m,w,l); }
    match m {
        0x0005=> { layout(STATE); 0 }, // WM_SIZE
        0x0007=> { if !(*STATE).hidden && !(*STATE).edit.is_null() { SetFocus((*STATE).edit); } 0 },
        0x0111 if (w>>16)&0xffff==0x0300 => { // EN_CHANGE, emitted by standard EDIT.
            if (*STATE).active { (*STATE).dirty=true; save(STATE); } 0
        },
        0x0133|0x0138=> { SetBkColor(w as Handle,BG); SetTextColor(w as Handle,FG); (*STATE).brush as isize },
        0x0010=> {
            if !save(STATE) {
                if !(*STATE).hidden { MessageBoxW(h,wide(&format!("保存できません。本文は画面に残しています。\n\n{}",(*STATE).last_error.as_deref().unwrap_or(""))).as_ptr(),wide("未保存").as_ptr(),0x10); }
                return 0;
            }
            DestroyWindow(h); 0
        },
        0x0002=> { PostQuitMessage(0); 0 },
        _=>DefWindowProcW(h,m,w,l)
    }
}

unsafe fn run()->Result<(),String> {
    let started=Instant::now();
    let args:Vec<String>=std::env::args().skip(1).collect();
    let test=args.iter().any(|a|a=="--test");
    let probe=args.iter().any(|a|a=="--probe");
    let hidden=test||probe;
    let explicit=args.iter().position(|a|a=="--data-dir").map(|i|args.get(i+1).ok_or("--data-dir requires a folder")).transpose()?;
    if hidden && explicit.is_none() { return Err("Hidden tests require --data-dir; personal drafts are never used.".into()); }
    let dir=match explicit { Some(p)=>PathBuf::from(p),None=>std::env::current_exe().map_err(|e|e.to_string())?.parent().and_then(Path::parent).ok_or("Expected executable in build/")?.join("data") };
    fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
    let _lock=lock(&dir).map_err(|e|format!("同じ原稿は既に開かれているか、保存先にアクセスできません。\n{e}"))?;
    let path=dir.join("native.txt");
    let disk=read_optional(&path).map_err(|e|format!("原稿を読めないため、編集を開始しません。\n{e}"))?;
    let initial=std::str::from_utf8(disk.as_deref().unwrap_or(b"")).map_err(|e|format!("UTF-8 として読めないため、編集を開始しません。\n{e}"))?;
    if initial.contains('\0') { return Err("原稿に NUL が含まれるため、編集を開始しません。".into()); }
    if test && !initial.is_empty() { return Err("--test requires an empty native.txt or an empty folder.".into()); }
    let initial=control_text(initial.strip_prefix('\u{feff}').unwrap_or(initial));
    SetProcessDPIAware();
    let instance=GetModuleHandleW(ptr::null());
    let brush=CreateSolidBrush(BG);
    let class=wide("MinimalEditorNativeV0");
    let wc=WndClass{style:3,proc:Some(window_proc),cls_extra:0,wnd_extra:0,instance,icon:ptr::null_mut(),cursor:LoadCursorW(ptr::null_mut(),32512usize as *const u16),background:brush,menu:ptr::null(),name:class.as_ptr()};
    if RegisterClassW(&wc)==0 { return Err(io::Error::last_os_error().to_string()); }
    let state=Box::into_raw(Box::new(State{edit:ptr::null_mut(),window:ptr::null_mut(),brush,font:ptr::null_mut(),path,disk,active:false,dirty:false,last_error:None,save_count:0,hidden}));
    STATE=state;
    (*state).window=CreateWindowExW(0,class.as_ptr(),wide(TITLE).as_ptr(),0x00cf0000,0x80000000u32 as i32,0x80000000u32 as i32,1040,800,ptr::null_mut(),ptr::null_mut(),instance,ptr::null_mut());
    if (*state).window.is_null() { return Err(io::Error::last_os_error().to_string()); }
    let dark=1i32; DwmSetWindowAttribute((*state).window,20,&dark as *const _ as *const c_void,4);
    (*state).edit=CreateWindowExW(0,wide("EDIT").as_ptr(),wide("").as_ptr(),0x40000000|0x10000000|0x00010000|0x0004|0x0040|0x1000,0,0,100,100,(*state).window,1usize as Handle,instance,ptr::null_mut());
    if (*state).edit.is_null() { return Err(io::Error::last_os_error().to_string()); }
    (*state).font=CreateFontW(-20,0,0,0,400,0,0,0,1,0,0,5,0,wide("BIZ UDGothic").as_ptr());
    SendMessageW((*state).edit,0x0030,(*state).font as usize,1); // WM_SETFONT
    SendMessageW((*state).edit,0x00c5,0x7ffffffe,0); // EM_SETLIMITTEXT; remove 32K default.
    if SetWindowTextW((*state).edit,wide(&initial).as_ptr())==0 { return Err("Could not load the draft into EDIT".into()); }
    if get_text((*state).edit).map_err(|e|e.to_string())? != normalized(&initial) { return Err("EDIT did not load the complete draft; editing stopped.".into()); }
    layout(state); (*state).active=true;
    if hidden {
        let ready_ms=started.elapsed().as_secs_f64()*1000.0;
        let mut replace_ms=0.0; let mut append_ms=0.0; let mut wm_char_ms=0.0;
        let mut wm_char_units=0;
        if test {
            let content="日本語の文章を書く。独立した道具。abc 123\n".repeat(500);
            let begin=Instant::now();
            SendMessageW((*state).edit,0x00c2,1,wide(&control_text(&content)).as_ptr() as isize); // EM_REPLACESEL
            replace_ms=begin.elapsed().as_secs_f64()*1000.0;
            if get_text((*state).edit).map_err(|e|e.to_string())? != content { return Err("EDIT text mismatch".into()); }
            if fs::read_to_string(&(*state).path).map_err(|e|e.to_string())? != content { return Err(format!("Initial save mismatch: {:?}",(*state).last_error)); }
            SendMessageW((*state).edit,0x00b1,usize::MAX,-1); // EM_SETSEL end
            let begin=Instant::now();
            SendMessageW((*state).edit,0x00c2,1,wide("追加の文字と絵文字 📝").as_ptr() as isize);
            append_ms=begin.elapsed().as_secs_f64()*1000.0;
            let mut expected=format!("{content}追加の文字と絵文字 📝");
            if get_text((*state).edit).map_err(|e|e.to_string())? != expected { return Err("Append text mismatch".into()); }
            if fs::read_to_string(&(*state).path).map_err(|e|e.to_string())? != expected { return Err("Append save mismatch".into()); }
            if fs::read_to_string((*state).path.with_extension("txt.bak")).map_err(|e|e.to_string())? != content { return Err("Backup mismatch".into()); }

            // Deliver character messages to this hidden EDIT only. This covers
            // the Windows character-input route, without global input/focus.
            let begin=Instant::now();
            for unit in "\r日本語abc".encode_utf16() {
                SendMessageW((*state).edit,0x0102,unit as usize,1); // WM_CHAR
                wm_char_units+=1;
            }
            expected.push_str("\n日本語abc");
            if get_text((*state).edit).map_err(|e|e.to_string())? != expected { return Err("WM_CHAR Japanese input mismatch".into()); }
            if fs::read_to_string(&(*state).path).map_err(|e|e.to_string())? != expected { return Err("WM_CHAR Japanese persistence mismatch".into()); }
            let surrogate:Vec<u16>="😀".encode_utf16().collect();
            SendMessageW((*state).edit,0x0102,surrogate[0] as usize,1);
            wm_char_units+=1;
            // An incomplete pair must never be saved with replacement characters.
            if fs::read_to_string(&(*state).path).map_err(|e|e.to_string())? != expected { return Err("Incomplete surrogate changed the saved draft".into()); }
            SendMessageW((*state).edit,0x0102,surrogate[1] as usize,1);
            wm_char_units+=1; expected.push('😀');
            if get_text((*state).edit).map_err(|e|e.to_string())? != expected || (*state).dirty || (*state).last_error.is_some() { return Err("Surrogate pair did not recover cleanly".into()); }
            if fs::read_to_string(&(*state).path).map_err(|e|e.to_string())? != expected { return Err("Surrogate pair persistence mismatch".into()); }
            wm_char_ms=begin.elapsed().as_secs_f64()*1000.0;

            // A blocked temporary pathname deterministically simulates a save
            // failure. Closing must preserve the live text; the next edit retries.
            let pending=(*state).path.with_extension("txt.pending");
            fs::create_dir(&pending).map_err(|e|e.to_string())?;
            SendMessageW((*state).edit,0x0102,'障' as usize,1); wm_char_units+=1;
            if !(*state).dirty || (*state).last_error.is_none() { return Err("Save failure was not exposed".into()); }
            if !get_text((*state).window).map_err(|e|e.to_string())?.starts_with("未保存") { return Err("Save failure title missing".into()); }
            if fs::read_to_string(&(*state).path).map_err(|e|e.to_string())? != expected { return Err("Failed save damaged original".into()); }
            expected.push('障');
            SendMessageW((*state).window,0x0010,0,0); // WM_CLOSE, hidden: no modal dialog.
            if get_text((*state).edit).map_err(|e|e.to_string())? != expected { return Err("Failed-save close lost live text".into()); }
            fs::remove_dir(&pending).map_err(|e|e.to_string())?;
            SendMessageW((*state).edit,0x0102,'害' as usize,1); wm_char_units+=1; expected.push('害');
            if (*state).dirty || (*state).last_error.is_some() { return Err("Save error did not recover".into()); }
            if fs::read_to_string(&(*state).path).map_err(|e|e.to_string())? != expected { return Err("Recovery save mismatch".into()); }

            let conflict=dir.join("storage-conflict.txt");
            fs::write(&conflict,b"external version").map_err(|e|e.to_string())?;
            if atomic_save(&conflict,b"unwanted overwrite",&Some(b"previous version".to_vec())).is_ok() { return Err("External-change guard accepted a stale version".into()); }
            if fs::read(&conflict).map_err(|e|e.to_string())? != b"external version" { return Err("External-change guard damaged the file".into()); }

            let mut key:Msg=std::mem::zeroed();
            key.hwnd=(*state).edit; key.message=0x0100; key.wparam=0x41;
            if editor_shortcut((*state).edit,&key,false,false) || editor_shortcut((*state).edit,&key,true,true) { return Err("Shortcut consumed unmodified A or AltGr+A".into()); }
            if !editor_shortcut((*state).edit,&key,true,false) { return Err("Ctrl+A was not handled".into()); }
            let mut selection_start=0u32; let mut selection_end=0u32;
            SendMessageW((*state).edit,0x00b0,&mut selection_start as *mut u32 as usize,&mut selection_end as *mut u32 as isize); // EM_GETSEL
            if selection_start!=0 || selection_end!=GetWindowTextLengthW((*state).edit) as u32 { return Err("Ctrl+A did not select the complete draft".into()); }
            key.wparam=0xe5; // VK_PROCESSKEY must reach the IME/default handler.
            if editor_shortcut((*state).edit,&key,true,false) { return Err("Shortcut intercepted VK_PROCESSKEY".into()); }
            key.message=0x010f; key.wparam=0x41; // WM_IME_COMPOSITION
            if editor_shortcut((*state).edit,&key,true,false) { return Err("Shortcut intercepted IME composition".into()); }
            SendMessageW((*state).edit,0x00b1,usize::MAX,-1);
        }
        let chars=get_text((*state).edit).map_err(|e|e.to_string())?.chars().count();
        let report=format!("{{\"ok\":true,\"hidden\":true,\"characters\":{chars},\"ready_ms\":{ready_ms:.3},\"replace_and_flush_ms\":{replace_ms:.3},\"append_and_flush_ms\":{append_ms:.3},\"wm_char_and_flush_ms\":{wm_char_ms:.3},\"wm_char_units\":{wm_char_units},\"surrogate_recovery\":{test},\"save_error_recovery\":{test},\"external_change_refused\":{test},\"ctrl_a_select_all\":{test},\"save_count\":{},\"input_path\":\"EM_REPLACESEL and Unicode WM_CHAR; no real IME or Handy exercised\"}}",(*state).save_count);
        fs::write(dir.join(if probe {"probe-result.json"} else {"test-result.json"}),&report).map_err(|e|e.to_string())?;
        println!("{report}");
        if let Some(i)=args.iter().position(|a|a=="--hold-ms") { let ms=args.get(i+1).ok_or("--hold-ms requires milliseconds")?.parse::<u64>().map_err(|e|e.to_string())?; std::thread::sleep(std::time::Duration::from_millis(ms)); }
        // Bypass WM_CLOSE: tests prove EN_CHANGE saved before any close handler.
        DestroyWindow((*state).window);
    } else {
        ShowWindow((*state).window,5); SetFocus((*state).edit);
        let mut msg:Msg=std::mem::zeroed();
        loop {
            let n=GetMessageW(&mut msg,ptr::null_mut(),0,0);
            if n==0 { break; }
            if n<0 { return Err(io::Error::last_os_error().to_string()); }
            if editor_shortcut((*state).edit,&msg,GetKeyState(0x11)<0,GetKeyState(0x12)<0) { continue; }
            TranslateMessage(&msg); DispatchMessageW(&msg);
        }
    }
    STATE=ptr::null_mut(); DeleteObject((*state).font); DeleteObject((*state).brush); drop(Box::from_raw(state));
    Ok(())
}
fn main() {
    if let Err(error)=unsafe { run() } {
        eprintln!("{error}");
        if !std::env::args().any(|a|a=="--test"||a=="--probe") { unsafe { MessageBoxW(ptr::null_mut(),wide(&error).as_ptr(),wide("Minimal Editor · Native").as_ptr(),0x10); } }
        std::process::exit(1);
    }
}
