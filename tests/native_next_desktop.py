"""End-to-end native editor tests on an undisplayed private Windows desktop."""
import ctypes as C
from ctypes import wintypes as W
import json
import shutil
from pathlib import Path
import subprocess
import sys
import time
import traceback
import gui_desktop as gui
from private_desktop import run, verify_desktop, screenshot
from memory_desktop import sample

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])


def child(folder, desktop):
    verify_desktop(desktop)
    exe = gui.ROOT / "build/native-next.exe"
    assert exe.is_file()
    data, report = folder / "data", folder / "state.json"
    process = None
    source = folder / "browser-copy-source"
    source.mkdir()
    (source / "web.txt").write_text("", encoding="utf-8")
    (source / "index.json").write_text(json.dumps({"version": 1, "lastId": "legacy", "memos": [
        {"id": "legacy", "createdAt": "2026-10-18T00:00:00.000Z"}]}), encoding="utf-8")
    subprocess.run(["node", str(gui.ROOT / "scripts/seed-native.mjs"), "--source-dir", str(source), "--target-dir", str(data)],
                   check=True, capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW)

    def launch():
        nonlocal process
        report.unlink(missing_ok=True)
        with (folder / "stdout.txt").open("wb") as out, (folder / "stderr.txt").open("wb") as err:
            process = subprocess.Popen([str(exe), "--test-mode", "--data-dir", str(data), "--report-file", str(report)],
                                       stdout=out, stderr=err, creationflags=subprocess.CREATE_NO_WINDOW)
        def ready():
            if process.poll() is not None:
                raise AssertionError(report.read_text(encoding="utf-8") if report.exists() else "Native exited before ready")
            # A reader can briefly block replacement of the instrumentation
            # file on Windows. Once the owned window is visible, request a
            # synchronous fresh snapshot before opening that file ourselves.
            owned = next((w for w in gui.windows() if w["pid"] == process.pid and w["class"].startswith("MinimalEditorNext-") and w["visible"]), None)
            if owned:
                send(owned["hwnd"], 0x8002, 0, 0)
            if report.exists():
                try:
                    state = json.loads(report.read_text(encoding="utf-8"))
                    return state if state.get("ready") else None
                except (ValueError, PermissionError):
                    return None
        return gui.eventually(ready, "Native not ready")

    def state():
        send(window, 0x8002, 0, 0)
        return json.loads(report.read_text(encoding="utf-8"))

    def key(vk, mods=0):
        send(window, 0x8001, vk, mods)
        return state()

    def insert(hwnd, text):
        value = C.create_unicode_buffer(text)
        send(hwnd, 0xC2, 1, C.cast(value, C.c_void_p).value)  # EM_REPLACESEL, undoable

    def title_text(hwnd, text):
        send(hwnd, 0xB1, 0, -1)
        insert(hwnd, text)

    try:
        first = launch()
        window, editor = first["window"], first["editor"]
        assert first["visible"] and not first["error"], first
        assert first["fontTwips"] == 300 and first["lineSpacingTwips"] == 540 and first["lineRule"] == 4, first
        assert first["formatRect"][1] == round(60 * first["scale"]), first
        initial_id = first["active"]
        text = "自分の言葉で書く。\n余白のある道具。Latin 123。\n" * 500
        started = time.monotonic()
        insert(editor, text)
        saved = state()
        input_ms = (time.monotonic() - started) * 1000
        assert saved["text"] == text and not saved["dirty"] and not saved["error"], saved
        path = data / "memos" / (initial_id + ".txt")
        assert path.read_text(encoding="utf-8") == text
        # Ordinary RichEdit Undo stays in the body, separate from archive undo.
        send(editor, 0xC7, 0, 0)
        assert state()["text"] == ""
        insert(editor, text)
        # WM_CHAR passes through the actual native input handler without global input.
        for character in "追記":
            send(editor, 0x102, ord(character), 0)
        assert state()["text"].endswith("追記")
        assert send(editor, 0xC6, 0, 0), "Body should have Undo history"
        key(0x50, 1)
        key(0x0D)
        assert send(editor, 0xC6, 0, 0), "Opening the current memo cleared Undo"
        screenshot(window, process.pid, folder / "body.png")
        body_memory = sample(process.pid)
        # Candidate selection does not activate until Enter, and outside click does not close.
        second = key(0x4E, 1)
        second_id = second["active"]
        assert second_id != initial_id and second["text"] == ""
        insert(editor, "二つ目のメモ")
        picker = key(0x50, 1)
        assert picker["picker"] == "normal" and len(picker["items"]) == 2
        send(picker["overlay"], 0x201, 1, (8 << 16) | 8)
        assert state()["picker"] == "normal"
        screenshot(window, process.pid, folder / "picker.png")
        renamed = key(0x4D, 1)
        assert renamed["renaming"]
        title_text(renamed["titleEditor"], "名前を変更")
        send(renamed["titleEditor"], 0x10D, 0, 0)
        assert key(0x0D)["renaming"], "IME Enter committed title prematurely"
        send(renamed["titleEditor"], 0x10E, 0, 0)
        # Synthetic start/end messages have no actual IME result string. Supply
        # the committed text explicitly; this tests the guard, not a real IME.
        title_text(renamed["titleEditor"], "名前を変更")
        assert not key(0x0D)["renaming"]
        assert any(m["title"] == "名前を変更" for m in state()["items"])
        key(0x4D, 1)
        title_text(state()["titleEditor"], "取消する変更")
        assert key(0x1B)["picker"] is None
        assert all(m["title"] != "取消する変更" for m in key(0x50, 1)["items"])
        archived = key(0x2E)
        assert len(archived["archives"]) == 1 and archived["undoArchive"] == second_id
        assert key(0x2E, 1)["archives"] == archived["archives"], "Ctrl+Delete archived another memo"
        assert key(0x2E, 2)["archives"] == archived["archives"], "Shift+Delete archived another memo"
        screenshot(window, process.pid, folder / "toast.png")
        undone = key(0x5A, 1)
        assert len(undone["archives"]) == 0 and len(undone["catalog"]) == 2
        archived = key(0x2E)
        assert len(archived["archives"]) == 1
        assert key(9)["toastFocused"], "Tab did not reach archive Undo"
        assert len(key(0x20)["archives"]) == 0, "Space did not activate archive Undo"
        archived = key(0x2E)
        assert len(archived["archives"]) == 1
        archived_picker = key(0x50, 3)
        assert archived_picker["picker"] == "archived" and len(archived_picker["items"]) == 1
        restored = key(0x0D)
        assert restored["picker"] is None and restored["active"] == second_id and restored["text"] == "二つ目のメモ"
        assert key(0x50, 3)["picker"] == "archived"
        assert key(0x50, 3)["picker"] is None
        literal = "{\\rtf1 これは書式命令ではなく本文。}"
        key(0x4E, 1)
        # Bulk EM_REPLACESEL in rich mode autodetects an RTF prefix; actual
        # typing must preserve those characters, as must document reload.
        for character in literal:
            send(editor, 0x102, ord(character), 0)
        assert state()["text"] == literal
        literal_id = state()["active"]
        key(0x50, 1)
        key(0x28)
        key(0x0D)
        picker = key(0x50, 1)
        at = next(i for i, memo in enumerate(picker["items"]) if memo["id"] == literal_id)
        for _ in range(picker["selected"] - at):
            key(0x26)
        for _ in range(at - picker["selected"]):
            key(0x28)
        assert key(0x0D)["text"] == literal, "Literal RTF was parsed as formatting"
        # All-archived state makes the body read-only; restoring a nonempty
        # memo and then creating again must successfully leave that state.
        all_picker = key(0x50, 1)
        for _ in range(len(all_picker["catalog"])):
            key(0x2E)
        empty = state()
        assert empty["active"] is None and empty["text"] == "" and not empty["catalog"]
        archived_picker = key(0x50, 3)
        at = next(i for i, memo in enumerate(archived_picker["items"]) if memo["id"] == initial_id)
        for _ in range(at - archived_picker["selected"]):
            key(0x28)
        for _ in range(archived_picker["selected"] - at):
            key(0x26)
        assert key(0x0D)["text"] == text + "追記"
        assert key(0x4E, 1)["text"] == ""
        insert(editor, "全件保管後の新規")
        assert state()["text"] == "全件保管後の新規" and not state()["dirty"]
        # Switch to the old long memo, save before kill, reopen the same active memo.
        picker = key(0x50, 1)
        at = next(i for i, memo in enumerate(picker["items"]) if memo["id"] == initial_id)
        for _ in range(picker["selected"] - at):
            key(0x26)
        for _ in range(at - picker["selected"]):
            key(0x28)
        opened = key(0x0D)
        assert opened["active"] == initial_id and opened["text"] == text + "追記"
        process.terminate()
        process.wait(timeout=5)
        reopened = launch()
        window, editor = reopened["window"], reopened["editor"]
        assert reopened["active"] == initial_id and reopened["text"] == text + "追記"
        # External edit conflict retains both external file and unsaved native buffer.
        path.write_text("外部で変更", encoding="utf-8")
        insert(editor, "未保存の追記")
        conflict = state()
        assert conflict["error"] and conflict["dirty"]
        assert conflict.get("recovery") and Path(conflict["recovery"]).read_text(encoding="utf-8") == conflict["text"]
        assert path.read_text(encoding="utf-8") == "外部で変更"
        before = conflict["text"]
        assert key(0x4E, 1)["active"] == initial_id
        assert state()["text"] == before
        process.terminate()
        process.wait(timeout=5)
        process = None
        # Exercise the real Node launcher with isolated data. Its GUI child
        # must remain visible despite the launch helper's hidden console.
        launch_app = folder / "launcher-app"
        for relative in ("ネイティブ版.vbs", "scripts/launch-native.mjs", "scripts/seed-native.mjs", "build/native-next.exe"):
            target = launch_app / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(gui.ROOT / relative, target)
        shutil.copytree(source, launch_app / "data")
        def launcher():
            subprocess.run(["wscript.exe", str(launch_app / "ネイティブ版.vbs")], check=True, capture_output=True,
                           creationflags=subprocess.CREATE_NO_WINDOW, timeout=10)
        def launched_windows():
            return [w for w in gui.windows() if w["class"].startswith("MinimalEditorNext-")]
        launcher()
        owned = gui.eventually(lambda: (values[0] if len(values := launched_windows()) == 1 and values[0]["visible"] else None),
                               "Native launcher did not show the editor")
        gui.show_window(owned["hwnd"], 6)
        gui.eventually(lambda: gui.iconic(owned["hwnd"]), "Owned native window did not minimize")
        launcher()
        gui.eventually(lambda: len(launched_windows()) == 1 and not gui.iconic(owned["hwnd"]),
                       "Second launch did not restore the same native window")
        assert (source / "web.txt").read_text(encoding="utf-8") == "", "Launcher changed browser source"
        gui.post_message(owned["hwnd"], 0x10, 0, 0)
        gui.eventually(lambda: not launched_windows(), "Owned launched editor did not close")
        return {"passed": True, "desktop": desktop, "text_characters": len(text), "insert_and_save_ms": input_ms,
                "body_memory": body_memory, "checks": ["native input and body Undo", "autosave", "current typography readback",
                    "new memo", "normal/archive modal", "IME title Enter guard", "rename/cancel", "archive/undo",
                    "restore and open", "outside click retained", "literal RTF roundtrip", "forced exit/reopen",
                    "all archived restores read-only body and creates again", "toast keyboard button",
                    "external conflict keeps buffer and recovery", "real launcher visible and same-window reopen"],
                "images": [str(folder / (n + ".png")) for n in ("body", "picker", "toast")],
                "real_ime_and_handy": "Requires human trial; no global input used"}
    finally:
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=5)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        folder = Path(sys.argv[2])
        try:
            result = child(folder, sys.argv[3])
        except BaseException:
            result = {"passed": False, "error": traceback.format_exc()}
        (folder / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        sys.exit(0 if result["passed"] else 1)
    else:
        run(__file__, "native-next-ui-")
