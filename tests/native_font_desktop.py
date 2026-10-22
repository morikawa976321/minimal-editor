"""Verify real per-character fonts on an undisplayed, owned Windows desktop."""
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import traceback
import gui_desktop as gui
from private_desktop import run, verify_desktop, screenshot

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])
open_process = gui.bind(gui.K, "OpenProcess", W.HANDLE, [W.DWORD, W.BOOL, W.DWORD])
allocate = gui.bind(gui.K, "VirtualAllocEx", C.c_void_p, [W.HANDLE, C.c_void_p, C.c_size_t, W.DWORD, W.DWORD])
free = gui.bind(gui.K, "VirtualFreeEx", W.BOOL, [W.HANDLE, C.c_void_p, C.c_size_t, W.DWORD])
write = gui.bind(gui.K, "WriteProcessMemory", W.BOOL, [W.HANDLE, C.c_void_p, C.c_void_p, C.c_size_t, C.POINTER(C.c_size_t)])
read = gui.bind(gui.K, "ReadProcessMemory", W.BOOL, [W.HANDLE, C.c_void_p, C.c_void_p, C.c_size_t, C.POINTER(C.c_size_t)])


def child(folder, desktop):
    verify_desktop(desktop)
    executable = Path(os.environ.get("MINIMAL_EDITOR_NATIVE_EXE", str(gui.ROOT / "build/native-next.exe")))
    report = folder / "state.json"
    process = None
    handle = memory = None
    measurements = []
    failures = []
    result = {"passed": False, "exe": str(executable), "measurements": measurements}
    try:
        process = subprocess.Popen([str(executable), "--test-mode", "--data-dir", str(folder / "data"),
                                    "--report-file", str(report)], creationflags=subprocess.CREATE_NO_WINDOW)

        def ready():
            assert process.poll() is None, "Owned editor exited"
            owned = next((w for w in gui.windows() if w["pid"] == process.pid and
                          w["class"].startswith("MinimalEditorNext-") and w["visible"]), None)
            if owned:
                send(owned["hwnd"], 0x8002, 0, 0)
                try:
                    state = json.loads(report.read_text(encoding="utf-8"))
                    return state if state.get("ready") else None
                except (ValueError, PermissionError):
                    pass

        first = gui.eventually(ready, "Owned editor not ready")
        window, editor = first["window"], first["editor"]
        handle = gui.checked(open_process(0x438, False, process.pid))
        memory = gui.checked(allocate(handle, None, 4096, 0x3000, 4))
        result["languageOptions"] = send(editor, 0x479, 0, 0)  # EM_GETLANGOPTIONS

        def put(raw, offset=0):
            buffer = C.create_string_buffer(raw)
            count = C.c_size_t()
            gui.checked(write(handle, memory + offset, buffer, len(raw), C.byref(count)))
            assert count.value == len(raw)

        def get(size, offset=0):
            buffer = C.create_string_buffer(size)
            count = C.c_size_t()
            gui.checked(read(handle, memory + offset, buffer, size, C.byref(count)))
            assert count.value == size
            return buffer.raw

        def point(index):
            put(bytes(8), 256)
            send(editor, 0x426, memory + 256, index)  # EM_POSFROMCHAR (RichEdit POINTL*)
            return struct.unpack("<ii", get(8, 256))

        def inspect(index):
            send(editor, 0xB1, index, index + 1)
            put(struct.pack("<I", 116) + bytes(112))
            send(editor, 0x43A, 1, memory)
            raw = get(116)
            return {"face": raw[26:90].decode("utf-16le").split("\0")[0],
                    "charset": raw[24], "twips": struct.unpack_from("<i", raw, 12)[0],
                    "weight": struct.unpack_from("<H", raw, 92)[0],
                    "width": point(index + 1)[0] - point(index)[0]}

        def settled():
            send(window, 0x8002, 0, 0)
            state = json.loads(report.read_text(encoding="utf-8"))
            return state if not (state.get("saving") or state.get("busy") or state.get("dirty")) else None

        def replace(text):
            send(editor, 0xB1, 0, -1)
            value = C.create_unicode_buffer(text)
            send(editor, 0xC2, 1, C.cast(value, C.c_void_p).value)
            return gui.eventually(settled, "Owned editor did not save")

        # Same 20 logical px, actual GDI font selection and advance: not a
        # guessed threshold or just the control's default face name.
        gdi = C.WinDLL("gdi32", use_last_error=True)
        create_dc = gui.bind(gdi, "CreateCompatibleDC", W.HDC, [W.HDC])
        create_font = gui.bind(gdi, "CreateFontW", W.HANDLE, [C.c_int] * 5 + [W.DWORD] * 8 + [W.LPCWSTR])
        select = gui.bind(gdi, "SelectObject", W.HANDLE, [W.HDC, W.HANDLE])
        delete = gui.bind(gdi, "DeleteObject", W.BOOL, [W.HANDLE])
        delete_dc = gui.bind(gdi, "DeleteDC", W.BOOL, [W.HDC])
        get_face = gui.bind(gdi, "GetTextFaceW", C.c_int, [W.HDC, C.c_int, W.LPWSTR])
        extent = gui.bind(gdi, "GetTextExtentPoint32W", W.BOOL, [W.HDC, W.LPCWSTR, C.c_int, C.c_void_p])
        dc = gui.checked(create_dc(None))
        font = gui.checked(create_font(-round(20 * first["scale"]), 0, 0, 0, 400,
                                       0, 0, 0, 0, 0, 0, 5, 0, "Intel One Mono"))
        old = select(dc, font)
        try:
            face = C.create_unicode_buffer(128)
            assert get_face(dc, 128, face) > 0
            size = (C.c_long * 2)()
            gui.checked(extent(dc, "n", 1, size))
            reference = {"face": face.value, "width": size[0]}
            assert reference["face"] == "Intel One Mono", reference
            result.update(reference=reference, scale=first["scale"])
        finally:
            select(dc, old)
            delete(font)
            delete_dc(dc)

        def check(label, index):
            actual = inspect(index)
            measurements.append({"case": label, **actual})
            if actual["face"] != "Intel One Mono" or actual["width"] != reference["width"]:
                screenshot(window, process.pid, folder / "wrong-font.png")
                failures.append(f"{label}: {actual}; reference={reference}")
            assert actual["twips"] == 300 and actual["weight"] == 400, actual

        # Selection inspection changes insertion style. Reset/reload each case
        # so that the probe cannot accidentally supply the next case's font.
        replace("")
        for char in "nnn":
            send(editor, 0x102, ord(char), 0)
        check("typed Latin in fresh memo", 1)
        replace("")
        for char in "日本語nnn":
            send(editor, 0x102, ord(char), 0)
        check("typed Latin immediately after Japanese", 4)
        replace("")
        text = "日本語から英数字へ。\n\nasdflja-lsdkfjo-ij-\n123487012938560987\n[][]{}{}{}<><><\n"
        # Type individual characters; insert paragraph boundaries explicitly
        # without global keyboard state or synthetic Return-key dependencies.
        for char in text:
            if char == "\n":
                paragraph = C.create_unicode_buffer("\r\n")
                send(editor, 0xC2, 1, C.cast(paragraph, C.c_void_p).value)
            else:
                send(editor, 0x102, ord(char), 0)
        saved = gui.eventually(settled, "Owned editor did not save typed sample")
        screenshot(window, process.pid, folder / "typed-sample.png")
        latin_at = text.index("asdflja")
        check("typed Latin after Japanese and empty paragraph", latin_at + 1)
        check("typed digits", text.index("123487") + 1)
        check("typed punctuation", text.index("[][]") + 1)
        # New memo and reopen exercise WM_SETTEXT/uniform_body_format instead
        # of attributing a successful insertion-only check to the load path.
        old_id = saved["active"]
        send(window, 0x8001, 0x4E, 1)
        gui.eventually(lambda: (value if (value := settled()) and value["active"] != old_id else None), "New memo not ready")
        send(window, 0x8001, 0x50, 1)
        send(window, 0x8001, 0x28, 0)
        send(window, 0x8001, 0x0D, 0)
        reopened = gui.eventually(settled, "Reopen did not finish")
        assert reopened["active"] == old_id and reopened["text"] == text, reopened
        check("reopened Latin", latin_at + 1)
        check("reopened digits", text.index("123487") + 1)
        screenshot(window, process.pid, folder / "reopened-sample.png")
        assert not failures, "\n".join(failures)
        result["passed"] = True
        result["real_ime"] = "WM_CHAR text and explicit paragraphs under Japanese layout; not physical IME candidate conversion"
    except BaseException:
        result["error"] = traceback.format_exc()
    finally:
        if memory:
            free(handle, memory, 0, 0x8000)
        if handle:
            gui.close(handle)
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
    return result


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        folder = Path(sys.argv[2])
        result = child(folder, sys.argv[3])
        (folder / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        sys.exit(0 if result["passed"] else 1)
    else:
        run(__file__, "native-font-ui-", timeout=30000)
