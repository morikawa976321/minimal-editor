"""Exercise the live typography toggle without touching the displayed desktop."""
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import subprocess
import sys
import traceback
import gui_desktop as gui
from private_desktop import run, verify_desktop, screenshot

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])
get_client = gui.bind(gui.U, "GetClientRect", W.BOOL, [W.HWND, C.c_void_p])
redraw = gui.bind(gui.U, "RedrawWindow", W.BOOL, [W.HWND, C.c_void_p, W.HANDLE, W.UINT])
map_point = gui.bind(gui.U, "MapWindowPoints", C.c_int, [W.HWND, W.HWND, C.c_void_p, W.UINT])
move = gui.bind(gui.U, "MoveWindow", W.BOOL, [W.HWND, C.c_int, C.c_int, C.c_int, C.c_int, W.BOOL])


class Point(C.Structure):
    _fields_ = [("x", C.c_long), ("y", C.c_long)]


child_at = gui.bind(gui.U, "ChildWindowFromPointEx", W.HWND, [W.HWND, Point, W.UINT])


def child(folder, desktop):
    verify_desktop(desktop)
    exe = Path(os.environ.get("MINIMAL_EDITOR_NATIVE_EXE", str(gui.ROOT / "build/native-next.exe")))
    data, report = folder / "data", folder / "state.json"
    (data / "memos").mkdir(parents=True)
    body = "自分の文章を書く。Latin 123。\n" * 500
    (data / "memos" / "memo.txt").write_text(body, encoding="utf-8")
    (data / "catalog.tsv").write_text("minimal-editor-native-v1\nactive\tmemo\nmemo\tmemo\t1000\t0\t" +
                                    "テスト temp.txt".encode("utf-8").hex() + "\n", encoding="utf-8")
    result = {"passed": False, "images": [], "checks": []}
    process = None
    try:
        process = subprocess.Popen([str(exe), "--test-mode", "--data-dir", str(data), "--report-file", str(report)],
                                   creationflags=subprocess.CREATE_NO_WINDOW)

        def ready():
            assert process.poll() is None
            owned = next((w for w in gui.windows() if w["pid"] == process.pid and
                          w["class"].startswith("MinimalEditorNext-") and w["visible"]), None)
            if owned:
                send(owned["hwnd"], 0x8002, 0, 0)
                value = json.loads(report.read_text(encoding="utf-8"))
                return value if value.get("ready") else None

        initial = gui.eventually(ready, "Toggle candidate did not load")
        window, editor, toggle = initial["window"], initial["editor"], initial["weightToggle"]
        result["initial_handles"] = {k: initial[k] for k in ("window", "editor", "weightToggle")}

        def state():
            send(window, 0x8002, 0, 0)
            return json.loads(report.read_text(encoding="utf-8"))

        def settled():
            return gui.eventually(lambda: (v if not v["busy"] and not v["saving"] and not v["dirty"] else None)
                                  if (v := state()) else None, "Persistence did not settle")

        def click(weight):
            assert process.poll() is None, f"Native exited: {process.returncode}"
            rect = (C.c_long * 4)()
            gui.checked(get_client(toggle, rect))
            x, y = rect[2] * (1 if weight == 400 else 3) // 4, rect[3] // 2
            point = Point(x, y)
            map_point(toggle, window, C.byref(point), 1)
            assert child_at(window, point, 1) == toggle, "Toggle is covered by another child"
            assert send(toggle, 0x21, 0, 0) == 3, "Toggle must not activate or steal focus"
            send(toggle, 0x201, 1, (y << 16) | x)
            send(toggle, 0x202, 0, (y << 16) | x)
            return state()

        def picture(name):
            gui.checked(redraw(window, None, None, 0x185))
            path = folder / f"{name}.png"
            screenshot(window, process.pid, path)
            result["images"].append(str(path))

        # Add an undoable edit, then change presentation without changing text,
        # selection, viewport, persistence generation, or the current focus.
        send(editor, 0xb1, 0, 0)
        addition = C.create_unicode_buffer("追記 ABC\n")
        send(editor, 0xc2, 1, C.cast(addition, C.c_void_p).value)
        settled()
        send(editor, 0xb1, 20, 50)
        send(editor, 0x20a, ((-120 & 0xffff) << 16), 0)
        before = state()
        result["before"] = {k: before[k] for k in ("window", "editor", "weightToggle", "ready")}
        picture("before-toggle")
        after = click(700)
        assert after["uiFontWeight"] == after["fontWeight"] == after["titleFontWeight"] == 700, after
        for field in ("text", "bodySelection", "bodyScroll", "generation", "saveCount", "focusedWindow"):
            assert after[field] == before[field], (field, before[field], after[field])
        assert not after["dirty"] and not after["error"]
        picture("body-700-toggle")
        send(editor, 0xc7, 0, 0)  # Text Undo must undo typing, not the display toggle.
        undone = settled()
        assert undone["text"] == body, "Presentation polluted the text Undo history"
        send(editor, 0x454, 0, 0)  # EM_REDO
        redone = settled()
        assert redone["text"] == before["text"], "Toggle destroyed text Redo history"
        assert redone["fontWeight"] == 700, "Text Redo restored the previous display weight"
        click(400)
        picture("body-400-toggle")
        result["checks"].append("body live weight, selection, scroll, focus, generation, Undo/Redo")

        # IME must retain control of its composition; switching is ignored until
        # composition ends, and clicking works immediately afterwards.
        send(editor, 0x10d, 0, 0)
        assert click(700)["uiFontWeight"] == 400
        send(editor, 0x10e, 0, 0)
        assert click(700)["uiFontWeight"] == 700
        result["checks"].append("IME composition is not interrupted")

        send(window, 0x8001, 0x50, 1)
        modal = state()
        assert modal["picker"] == "normal" and not modal["bodyVisible"]
        changed = click(400)
        assert changed["picker"] == "normal" and not changed["bodyVisible"]
        assert changed["uiFontWeight"] == 400 and changed["focusedWindow"] == modal["focusedWindow"]
        picture("picker-toggle")
        send(window, 0x8001, 0x4d, 1)
        renaming = state()
        assert renaming["renaming"]
        assert click(700)["titleFontWeight"] == 700
        assert state()["titleText"] == renaming["titleText"]
        picture("rename-toggle")
        send(window, 0x8001, 0x1b, 0)
        send(window, 0x8001, 0x4e, 1)
        fresh = settled()
        assert fresh["active"] != "memo" and fresh["fontWeight"] == 700
        typed = C.create_unicode_buffer("新しい本文 ABC 123")
        send(editor, 0xc2, 1, C.cast(typed, C.c_void_p).value)
        assert settled()["fontWeight"] == 700
        gui.checked(move(window, 0, 0, 460, 620, True))
        assert click(400)["uiFontWeight"] == 400
        picture("narrow-toggle")
        result["checks"].append("toggle visible above modal and rename, narrow window hit testing")
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
        run(__file__, "native-weight-toggle-", timeout=60000)
