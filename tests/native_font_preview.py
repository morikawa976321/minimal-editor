"""Capture actual native typography candidates on an undisplayed desktop."""
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import traceback
import gui_desktop as gui
from private_desktop import run, verify_desktop, screenshot

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])
move = gui.bind(gui.U, "MoveWindow", W.BOOL, [W.HWND, C.c_int, C.c_int, C.c_int, C.c_int, W.BOOL])
redraw = gui.bind(gui.U, "RedrawWindow", W.BOOL, [W.HWND, C.c_void_p, W.HANDLE, W.UINT])


def child(folder, desktop):
    verify_desktop(desktop)
    result = {"passed": False, "captures": [], "formats": []}
    process = None
    try:
        for weight, name in ((400, "native-next.exe"), (500, "native-font-500.exe"), (700, "native-font-700.exe")):
            data = folder / str(weight)
            (data / "memos").mkdir(parents=True)
            titles = ["temp.txtから テスト 123", "文章を書く o r u n m f", "保管したメモ Archive 456"]
            body = "自分の言葉で、静かに文章を書く。\n余白のある道具。日本語と英数字。\nIntel One Mono / BIZ UDGothic\nfont 20px 0123456789 o r u n m f\n"
            lines = ["minimal-editor-native-v1", "active\tmemo0"]
            for i, title in enumerate(titles):
                lines.append(f"memo\tmemo{i}\t{1000 + i}\t{int(i == 2)}\t{title.encode('utf-8').hex()}")
                (data / "memos" / f"memo{i}.txt").write_text(body, encoding="utf-8")
            (data / "catalog.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8")
            report = data / "state.json"
            executable = os.environ.get(f"MINIMAL_EDITOR_FONT_{weight}_EXE", str(gui.ROOT / "build" / name))
            process = subprocess.Popen([executable, "--test-mode", "--data-dir", str(data),
                                        "--report-file", str(report)], creationflags=subprocess.CREATE_NO_WINDOW)

            def ready():
                assert process.poll() is None
                owned = next((w for w in gui.windows() if w["pid"] == process.pid and
                              w["class"].startswith("MinimalEditorNext-") and w["visible"]), None)
                if owned:
                    send(owned["hwnd"], 0x8002, 0, 0)
                    value = json.loads(report.read_text(encoding="utf-8"))
                    return value if value.get("ready") else None

            first = gui.eventually(ready, "Preview did not start")
            window = first["window"]
            gui.checked(move(window, 0, 0, 960, 650, True))

            def state():
                send(window, 0x8002, 0, 0)
                return json.loads(report.read_text(encoding="utf-8"))

            def key(vk, mods=0):
                send(window, 0x8001, vk, mods)
                return gui.eventually(lambda: (v if not v["busy"] and not v["saving"] else None)
                                      if (v := state()) else None, "Preview action did not settle")

            def capture(mode):
                value = state()
                expected = {"picker": "normal", "rename": "normal", "archive": "archived", "toast": "normal"}.get(mode)
                assert value["picker"] == expected, value
                if mode == "body":
                    # Hide the owned RichEdit caret so its blink phase cannot
                    # masquerade as a difference in the font comparison.
                    send(value["editor"], 0x8, 0, 0)  # WM_KILLFOCUS
                time.sleep(0.08)
                gui.checked(redraw(window, None, None, 0x185))
                if expected:
                    gui.checked(redraw(value["overlay"], None, None, 0x185))
                path = folder / f"{mode}-{weight}.png"
                screenshot(window, process.pid, path)
                result["captures"].append({"weight": weight, "mode": mode, "path": str(path)})

            fmt = state()
            assert fmt["fontTwips"] == fmt["titleFontTwips"] == 300, fmt
            assert fmt["fontWeight"] == fmt["titleFontWeight"] == weight, fmt
            assert fmt["uiFontPixels"] == 20 and fmt["uiFontWeight"] == weight, fmt
            result["formats"].append({k: fmt[k] for k in ("font", "fontTwips", "fontWeight", "titleFontTwips", "titleFontWeight", "scale")})
            capture("body")
            key(0x50, 1)
            capture("picker")
            key(0x4D, 1)
            capture("rename")
            key(0x1B)
            key(0x50, 3)
            capture("archive")
            key(0x1B)
            key(0x50, 1)
            key(0x2E)
            capture("toast")
            process.terminate()
            process.wait(timeout=5)
            process = None
        result["identical_screenshots"] = {mode: (folder / f"{mode}-400.png").read_bytes() ==
                                         (folder / f"{mode}-500.png").read_bytes()
                                         for mode in ("body", "picker", "rename", "archive", "toast")}
        result["700_matches_400"] = {mode: (folder / f"{mode}-400.png").read_bytes() ==
                                    (folder / f"{mode}-700.png").read_bytes()
                                    for mode in ("body", "picker", "rename", "archive", "toast")}
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
        run(__file__, "native-font-preview-", timeout=60000)
