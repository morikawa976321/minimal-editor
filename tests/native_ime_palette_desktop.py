"""Check native selection drawing during synthetic composition notifications.

This exercises the real control's palette, not a physical IME candidate session.
Only owned windows on an undisplayed desktop and isolated data are used.
"""
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import traceback
import zlib
import gui_desktop as gui
from private_desktop import run, verify_desktop, screenshot

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])
sys_color = gui.bind(gui.U, "GetSysColor", W.DWORD, [C.c_int])


def colors(path):
    png = path.read_bytes()
    at, data = 8, b""
    while at < len(png):
        size = struct.unpack_from(">I", png, at)[0]
        if png[at + 4:at + 8] == b"IDAT":
            data += png[at + 8:at + 8 + size]
        at += size + 12
    width, height = struct.unpack_from(">II", png, 16)
    raw = zlib.decompress(data)
    counts = {"blue": 0, "gray": 0}
    highlight = sys_color(13)
    highlight_rgb = (highlight & 255, (highlight >> 8) & 255, (highlight >> 16) & 255)
    for y in range(height):
        row = raw[y * (width * 3 + 1) + 1:(y + 1) * (width * 3 + 1)]
        for x in range(0, len(row), 3):
            r, g, b = row[x:x + 3]
            counts["blue"] += int((r, g, b) == highlight_rgb)
            counts["gray"] += int((r, g, b) == (65, 65, 65))
    return counts


def child(folder, desktop):
    verify_desktop(desktop)
    exe = Path(os.environ.get("MINIMAL_EDITOR_NATIVE_EXE", str(gui.ROOT / "build/native-next.exe")))
    data, report = folder / "data", folder / "state.json"
    (data / "memos").mkdir(parents=True)
    text = "見立ての研究\n\n見立て"
    (data / "memos/probe.txt").write_text(text, encoding="utf-8")
    (data / "catalog.tsv").write_text("minimal-editor-native-v1\nactive\tprobe\nmemo\tprobe\t1\t0\t-\n", encoding="utf-8")
    result = {"passed": False, "images": [], "measurements": {},
              "scope": "Native selection plus synthetic WM_IME_START/END; actual IME candidate conversion remains unverified"}
    process = None
    system_before = [sys_color(13), sys_color(14)]

    def state():
        send(window, 0x8002, 0, 0)
        return json.loads(report.read_text(encoding="utf-8"))

    def image(name):
        path = folder / f"{name}.png"
        screenshot(window, process.pid, path)
        result["images"].append(str(path))
        result["measurements"][name] = colors(path)

    try:
        process = subprocess.Popen([str(exe), "--test-mode", "--data-dir", str(data), "--report-file", str(report)],
                                   creationflags=subprocess.CREATE_NO_WINDOW)
        def ready():
            assert process.poll() is None
            owned = next((w for w in gui.windows() if w["pid"] == process.pid and w["class"].startswith("MinimalEditorNext-") and w["visible"]), None)
            if owned:
                send(owned["hwnd"], 0x8002, 0, 0)
                value = json.loads(report.read_text(encoding="utf-8"))
                return value if value.get("ready") else None
        first = gui.eventually(ready, "Owned app did not load")
        window, editor = first["window"], first["editor"]
        send(editor, 0xb1, 8, 11)
        before = state()
        image("normal-selection")
        send(editor, 0xb1, -1, -1)  # Starting an IME with a selection replaces it.
        send(editor, 0x10d, 0, 0)
        # Force a fresh native highlight render, retaining the same range.
        send(editor, 0xb1, 8, 11)
        image("composition-selection")
        during = state()
        assert during["composing"] and during["bodySelection"] == before["bodySelection"]
        send(editor, 0x10e, 0, 0)
        send(editor, 0xb1, 8, 11)
        image("after-composition")
        after = state()
        assert not after["composing"]
        assert after["text"] == during["text"] == before["text"] == text
        assert after["bodySelection"] == before["bodySelection"]
        assert after["saveCount"] == before["saveCount"] and not after["dirty"]
        send(window, 0x8001, 0x50, 1)
        send(window, 0x8001, 0x4d, 1)
        renamed = state()
        assert renamed["renaming"]
        title = renamed["titleEditor"]
        send(title, 0xb1, -1, -1)
        send(title, 0x10d, 0, 0)
        send(title, 0xb1, 0, 3)
        image("title-composition")
        send(title, 0x10e, 0, 0)
        send(window, 0x8001, 0x1b, 0)
        result["system_colors_before"] = system_before
        result["system_colors_after"] = [sys_color(13), sys_color(14)]
        assert result["system_colors_after"] == system_before
        assert all(m["blue"] == 0 for m in result["measurements"].values()), result["measurements"]
        assert result["measurements"]["normal-selection"]["gray"] > 0, result["measurements"]
        result["passed"] = True
    except BaseException:
        result["error"] = traceback.format_exc()
    finally:
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
        run(__file__, "native-ime-palette-", timeout=60000)
