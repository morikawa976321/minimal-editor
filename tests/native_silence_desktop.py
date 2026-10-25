"""Verify RichEdit warning paths on an owned, undisplayed desktop.

The test-only import counter replaces MessageBeep in the owned child's memory;
it counts attempted calls without playing sounds or changing system settings.
"""
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
from native_beep_probe import install
from private_desktop import run, verify_desktop, screenshot

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])


def child(folder, desktop):
    verify_desktop(desktop)
    exe = Path(os.environ.get("MINIMAL_EDITOR_NATIVE_EXE", str(gui.ROOT / "build/native-next.exe")))
    data, report = folder / "data", folder / "state.json"
    (data / "memos").mkdir(parents=True)
    texts = {"multi": "最初の行。\n中間の行。\n最後の行🙂", "short": "一行だけ🙂", "empty": ""}
    lines = ["minimal-editor-native-v1", "active\tmulti"]
    for i, (memo_id, text) in enumerate(texts.items()):
        lines.append(f"memo\t{memo_id}\t{1000 + i}\t0\t{memo_id.encode().hex()}")
        (data / "memos" / f"{memo_id}.txt").write_text(text, encoding="utf-8")
    (data / "catalog.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8")
    result = {"passed": False, "executable": str(exe), "cases": [], "observations": {}}
    process = close_probe = None

    def state():
        send(window, 0x8002, 0, 0)
        return json.loads(report.read_text(encoding="utf-8"))

    def settled():
        return gui.eventually(lambda: (s if not (s := state())["busy"] and not s["saving"] else None),
                              "Pending action did not finish")

    def key(vk, mods=0):
        send(window, 0x8001, vk, mods)
        return settled()

    def case(name, hwnd, messages):
        prior, before = count(), state()
        for msg, w, l in messages:
            send(hwnd, msg, w, l)
        after = state()
        calls = count() - prior
        result["cases"].append({"name": name, "beeps": calls})
        assert after["text"] == before["text"], name
        return after

    def boundaries(name, hwnd):
        for edge, position, keys in (("start", 0, (0x25, 0x26, 0x24, 0x21)),
                                     ("end", -1, (0x27, 0x28, 0x23, 0x22))):
            send(hwnd, 0xB1, position, position)
            for vk in keys:
                case(f"{name} {edge} key {vk:02x}", hwnd, [(0x100, vk, 0)] * 3)
            # A backspace is WM_CHAR; Delete is WM_KEYDOWN.
            case(f"{name} {edge} delete", hwnd,
                 [(0x102, 8, 0)] if edge == "start" else [(0x100, 0x2e, 0)])
        for msg in (0xC7, 0x454):  # Empty Undo and Redo histories.
            case(f"{name} history {msg:03x}", hwnd, [(msg, 0, 0)] * 3)
        case(f"{name} Ctrl+Z/Ctrl+Y characters", hwnd, [(0x102, 0x1a, 0), (0x102, 0x19, 0)])

    def open_memo(memo_id):
        picker = key(0x50, 1)
        index = next(i for i, item in enumerate(picker["items"]) if item["id"] == memo_id)
        for _ in range(abs(index - picker["selected"])):
            key(0x28 if index > picker["selected"] else 0x26)
        opened = key(0x0d)
        assert opened["active"] == memo_id and opened["picker"] is None

    try:
        with (folder / "stdout.txt").open("wb") as out, (folder / "stderr.txt").open("wb") as err:
            process = subprocess.Popen([str(exe), "--test-mode", "--data-dir", str(data), "--report-file", str(report)],
                                       stdout=out, stderr=err, creationflags=subprocess.CREATE_NO_WINDOW)
        def ready():
            assert process.poll() is None, "App exited before ready"
            owned = next((w for w in gui.windows() if w["pid"] == process.pid and w["class"].startswith("MinimalEditorNext-") and w["visible"]), None)
            if owned:
                send(owned["hwnd"], 0x8002, 0, 0)
                value = json.loads(report.read_text(encoding="utf-8"))
                return value if value.get("ready") else None
        first = gui.eventually(ready, "App did not become ready")
        window, editor = first["window"], first["editor"]
        count, close_probe = install(process.pid)
        send(editor, 0xB1, -1, -1)
        end = state()["bodySelection"]
        result["observations"]["multiline_end"] = {"selection": end, "wm_gettextlength": send(editor, 0x0e, 0, 0)}
        after = case("multiline last position Down repeated", editor, [(0x100, 0x28, 0)] * 10)
        assert after["bodySelection"] == end
        for memo_id in texts:
            if memo_id != "multi":
                open_memo(memo_id)
            boundaries(memo_id, editor)
        # Read-only input must stay silent without allowing edits.
        send(editor, 0xcf, 1, 0)
        case("read-only character", editor, [(0x102, ord('x'), 0)])
        case("read-only delete", editor, [(0x100, 0x2e, 0), (0x102, 8, 0)])
        send(editor, 0xcf, 0, 0)
        # Synthetic composition notifications bypass our navigation remapping,
        # exercising the native engine; they do not simulate real IME candidates.
        send(editor, 0x10d, 0, 0)
        case("composition native boundaries", editor, [(0x100, vk, 0) for vk in (0x26, 0x28, 0x25, 0x27)])
        send(editor, 0x10e, 0, 0)
        key(0x50, 1)
        renamed = key(0x4d, 1)
        assert renamed["renaming"]
        title = renamed["titleEditor"]
        send(title, 0xcd, 0, 0)  # Clear only the isolated title's Undo history.
        boundaries("title", title)
        # Typing past a title limit used to beep independently of navigation.
        send(title, 0x435, 0, 1)
        send(title, 0xb1, -1, -1)
        case("title maximum length", title, [(0x102, ord('x'), 0)] * 3)
        key(0x1b)
        # Native menu processing can beep independently of the text engine.
        assert send(window, 0x106, ord('q'), 1 << 29) == 0
        assert send(window, 0x120, ord('q'), 0) == 1 << 16
        assert state()["picker"] is None and not state()["error"]
        result["observations"]["unassigned_system_character"] = "consumed"
        result["observations"]["unmatched_menu_character"] = "MNC_CLOSE"
        result["message_beep_calls"] = count()
        assert result["message_beep_calls"] == 0, result["cases"]
        assert not state()["error"]
        # A real startup failure must display its message without MessageBox.
        # Observe sound events only from this new, owned process, before resuming it.
        blocked = folder / "blocked-data-path"
        blocked.write_text("not a directory", encoding="utf-8")
        fatal_report = folder / "fatal.json"
        si, pi = gui.StartupInfo(), gui.ProcessInfo()
        si.cb = C.sizeof(si)
        command = C.create_unicode_buffer(subprocess.list2cmdline(
            [str(exe), "--data-dir", str(blocked), "--report-file", str(fatal_report)]))
        gui.checked(gui.create_process(str(exe), command, None, None, False, 0x08000004,
                                       None, str(gui.ROOT), C.byref(si), C.byref(pi)))
        events = []
        callback_type = C.WINFUNCTYPE(None, W.HANDLE, W.DWORD, W.HWND, C.c_long, C.c_long, W.DWORD, W.DWORD)
        callback = callback_type(lambda *args: events.append(list(args[1:])))
        hook_event = gui.bind(gui.U, "SetWinEventHook", W.HANDLE,
                              [W.DWORD, W.DWORD, W.HMODULE, callback_type, W.DWORD, W.DWORD, W.DWORD])
        unhook = gui.bind(gui.U, "UnhookWinEvent", W.BOOL, [W.HANDLE])
        hook = None
        try:
            hook = gui.checked(hook_event(1, 1, None, callback, pi.pid, 0, 0))
            assert gui.resume(pi.thread) != 0xffffffff
            dialog = gui.eventually(lambda: next((w for w in gui.windows() if w["pid"] == pi.pid and
                                                  w["class"] == "#32770" and w["visible"]), None),
                                    "Silent startup error dialog did not appear")
            assert "fatal" in json.loads(fatal_report.read_text(encoding="utf-8"))
            screenshot(dialog["hwnd"], pi.pid, folder / "startup-error.png")
            send(dialog["hwnd"], 0x106, ord('q'), 1 << 29)
            assert send(dialog["hwnd"], 0x120, ord('q'), 0) == 1 << 16
            send(dialog["hwnd"], 0x111, 1, 0)  # OK on the owned dialog only.
            assert gui.wait(pi.process, 5000) == 0
            peek = gui.bind(gui.U, "PeekMessageW", W.BOOL, [C.POINTER(W.MSG), W.HWND, W.UINT, W.UINT, W.UINT])
            dispatch = gui.bind(gui.U, "DispatchMessageW", C.c_ssize_t, [C.POINTER(W.MSG)])
            deadline, msg = time.monotonic() + 0.15, W.MSG()
            while time.monotonic() < deadline:
                while peek(C.byref(msg), None, 0, 0, 1):
                    dispatch(C.byref(msg))
                time.sleep(0.01)
            result["observations"]["startup_error_sound_events"] = events
            assert not events, events
        finally:
            if hook:
                unhook(hook)
            if gui.wait(pi.process, 0) != 0:
                gui.terminate(pi.process, 1)
                gui.wait(pi.process, 5000)
            gui.close(pi.thread)
            gui.close(pi.process)
        result["passed"] = True
    except BaseException:
        result["error"] = traceback.format_exc()
    finally:
        if close_probe:
            close_probe()
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
        run(__file__, "native-silence-ui-", timeout=60000)
