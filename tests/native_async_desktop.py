"""Inject slow disk saves and verify GUI progress on an undisplayed desktop.

The latency bound is a test condition relative to the injected 1 s delay,
not a product performance guarantee on arbitrary PCs or storage devices.
"""
import ctypes as C
from ctypes import wintypes as W
import json
from pathlib import Path
import subprocess
import sys
import time
import traceback
import gui_desktop as gui
from private_desktop import run, verify_desktop

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t,
                [W.HWND, W.UINT, W.WPARAM, W.LPARAM])
DELAY_MS = 1000
RESPONSE_BOUND_MS = DELAY_MS * 0.6


def child(folder, desktop):
    verify_desktop(desktop)
    exe = gui.ROOT / "build/native-next.exe"
    data, report = folder / "data", folder / "state.json"
    source = folder / "source"
    source.mkdir()
    (source / "web.txt").write_text("", encoding="utf-8")
    subprocess.run(["node", str(gui.ROOT / "scripts/seed-native.mjs"),
                    "--source-dir", str(source), "--target-dir", str(data)],
                   check=True, capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW)
    process = None
    timings = {}

    def state():
        send(window, 0x8002, 0, 0)
        return json.loads(report.read_text(encoding="utf-8"))

    def until(predicate, message, seconds=10):
        def check():
            value = state()
            return value if predicate(value) else None
        return gui.eventually(check, message, seconds=seconds)

    def clean():
        return until(lambda s: not s["busy"] and not s["saving"] and not s["dirty"],
                     "Pending save/operation did not finish")

    def insert(text):
        send(editor, 0xB1, -1, -1)
        value = C.create_unicode_buffer(text)
        send(editor, 0xC2, 1, C.cast(value, C.c_void_p).value)

    def key(vk, mods=0):
        send(window, 0x8001, vk, mods)

    try:
        with (folder / "stdout.txt").open("wb") as out, (folder / "stderr.txt").open("wb") as err:
            process = subprocess.Popen([str(exe), "--test-mode", "--data-dir", str(data),
                                        "--report-file", str(report), "--test-save-delay-ms", str(DELAY_MS)],
                                       stdout=out, stderr=err, creationflags=subprocess.CREATE_NO_WINDOW)

        def ready():
            assert process.poll() is None, "Native exited before ready"
            owned = next((w for w in gui.windows() if w["pid"] == process.pid and
                          w["class"].startswith("MinimalEditorNext-") and w["visible"]), None)
            if not owned:
                return None
            send(owned["hwnd"], 0x8002, 0, 0)
            if report.exists():
                value = json.loads(report.read_text(encoding="utf-8"))
                return value if value.get("ready") else None
        first = gui.eventually(ready, "Native not ready")
        window, editor = first["window"], first["editor"]
        original_id = first["active"]
        original_path = data / "memos" / (original_id + ".txt")
        clean()

        # Start generation A, then type generation B while A is delayed.
        started = time.monotonic()
        insert("一回目")
        a = until(lambda s: s["saving"], "Delayed save did not start")
        timings["first_input_and_snapshot_ms"] = (time.monotonic() - started) * 1000
        assert a["dirty"] and a["text"] == "一回目", a
        started = time.monotonic()
        for character in "追記":
            send(editor, 0x102, ord(character), 0)
        b = state()
        timings["input_during_save_and_snapshot_ms"] = (time.monotonic() - started) * 1000
        assert b["text"] == "一回目追記" and b["dirty"] and b["saving"], b
        assert b["generation"] > a["generation"] >= b["savedGeneration"], (a, b)
        assert timings["first_input_and_snapshot_ms"] < RESPONSE_BOUND_MS, timings
        assert timings["input_during_save_and_snapshot_ms"] < RESPONSE_BOUND_MS, timings
        acknowledged_a = until(lambda s: s["savedGeneration"] >= a["generation"],
                               "First delayed save was not acknowledged")
        assert acknowledged_a["savedGeneration"] < b["generation"], acknowledged_a
        assert acknowledged_a["dirty"] and acknowledged_a["text"] == b["text"], acknowledged_a
        saved = clean()
        assert saved["savedGeneration"] == saved["generation"], saved
        assert original_path.read_text(encoding="utf-8") == "一回目追記"

        # Ctrl+N waits for the latest body save before creating another memo.
        insert("新規前")
        until(lambda s: s["saving"], "Save before new did not start")
        started = time.monotonic()
        key(0x4E, 1)
        pending_new = state()
        timings["new_request_and_snapshot_ms"] = (time.monotonic() - started) * 1000
        assert pending_new["active"] == original_id and pending_new["busy"], pending_new
        assert timings["new_request_and_snapshot_ms"] < RESPONSE_BOUND_MS, timings
        insert("新規待機中の追記")
        while_new_pending = state()
        assert while_new_pending["busy"] and while_new_pending["text"].endswith("新規待機中の追記"), while_new_pending
        second = until(lambda s: s["active"] != original_id and not s["busy"], "New memo never opened")
        second_id = second["active"]
        second_path = data / "memos" / (second_id + ".txt")
        original_text = "一回目追記新規前新規待機中の追記"
        assert original_path.read_text(encoding="utf-8") == original_text

        # Selecting a different memo follows the same save-before-switch rule.
        insert("二つ目の本文")
        until(lambda s: s["saving"], "Save before switch did not start")
        key(0x50, 1)
        picker = state()
        target = next(i for i, memo in enumerate(picker["items"]) if memo["id"] == original_id)
        for _ in range(picker["selected"] - target):
            key(0x26)
        for _ in range(target - picker["selected"]):
            key(0x28)
        key(0x0D)
        pending_switch = state()
        assert pending_switch["active"] == second_id and pending_switch["busy"], pending_switch
        insert("切替待機中の追記")
        while_switch_pending = state()
        assert while_switch_pending["busy"] and while_switch_pending["text"].endswith("切替待機中の追記"), while_switch_pending
        switched = until(lambda s: s["active"] == original_id and not s["busy"], "Memo switch never completed")
        assert switched["text"] == original_text, switched
        assert second_path.read_text(encoding="utf-8") == "二つ目の本文切替待機中の追記"

        # External conflict preserves both copies and leaves typing responsive.
        original_path.write_text("外部の本文", encoding="utf-8")
        insert("競合する追記")
        conflict = until(lambda s: bool(s["error"]) and not s["saving"], "Conflict was not detected")
        assert conflict["dirty"] and conflict.get("recovery"), conflict
        assert Path(conflict["recovery"]).read_text(encoding="utf-8") == conflict["text"]
        started = time.monotonic()
        insert("競合後も入力")
        after_conflict = state()
        timings["input_after_conflict_and_snapshot_ms"] = (time.monotonic() - started) * 1000
        assert after_conflict["text"] == conflict["text"] + "競合後も入力", after_conflict
        assert after_conflict["dirty"] and after_conflict["generation"] > conflict["generation"], after_conflict
        assert timings["input_after_conflict_and_snapshot_ms"] < RESPONSE_BOUND_MS, timings
        recovered = until(lambda s: bool(s.get("recovery")) and not s["saving"] and
                          Path(s["recovery"]).read_text(encoding="utf-8") == after_conflict["text"],
                          "Latest conflicted input was not recovered")
        assert original_path.read_text(encoding="utf-8") == "外部の本文"
        key(0x4E, 1)
        failed_transition = until(lambda s: not s["busy"] and not s["saving"], "Failed transition stayed busy")
        assert failed_transition["active"] == original_id and failed_transition["dirty"], failed_transition

        # End conflicted instance without replacing the external file. Reopen
        # from that file to test orderly close on a clean persistence baseline.
        process.terminate()
        process.wait(timeout=5)
        report.unlink(missing_ok=True)
        with (folder / "close-stdout.txt").open("wb") as out, (folder / "close-stderr.txt").open("wb") as err:
            process = subprocess.Popen([str(exe), "--test-mode", "--data-dir", str(data),
                                        "--report-file", str(report), "--test-save-delay-ms", str(DELAY_MS)],
                                       stdout=out, stderr=err, creationflags=subprocess.CREATE_NO_WINDOW)
        reopened = gui.eventually(ready, "Reopened native not ready")
        window, editor = reopened["window"], reopened["editor"]
        assert reopened["text"] == "外部の本文", reopened
        clean()
        insert("終了前の追記")
        until(lambda s: s["saving"], "Save before close did not start")
        send(editor, 0x10D, 0, 0)
        assert state()["composing"], "Synthetic IME start was not registered"
        started = time.monotonic()
        send(window, 0x10, 0, 0)
        closing = state()
        timings["close_request_and_snapshot_ms"] = (time.monotonic() - started) * 1000
        assert process.poll() is None and closing["busy"], closing
        assert timings["close_request_and_snapshot_ms"] < RESPONSE_BOUND_MS, timings
        insert("終了待機中の確定文字")
        latest_close_text = "外部の本文終了前の追記終了待機中の確定文字"
        # These start/end messages exercise composition guards only. They do
        # not emulate an actual IME candidate window or composition result.
        time.sleep(DELAY_MS / 1000 + 0.1)
        during_composition = state()
        assert process.poll() is None and during_composition["busy"] and during_composition["composing"], during_composition
        assert during_composition["dirty"] and during_composition["text"] == latest_close_text, during_composition
        send(editor, 0x10E, 0, 0)
        process.wait(timeout=8)
        assert process.returncode == 0
        assert original_path.read_text(encoding="utf-8") == latest_close_text

        # A close request must survive a structural operation already sent to
        # the worker; it cannot be dropped merely because the UI is busy.
        report.unlink(missing_ok=True)
        with (folder / "action-close-stdout.txt").open("wb") as out, (folder / "action-close-stderr.txt").open("wb") as err:
            process = subprocess.Popen([str(exe), "--test-mode", "--data-dir", str(data),
                                        "--report-file", str(report)],
                                       stdout=out, stderr=err, creationflags=subprocess.CREATE_NO_WINDOW)
        reopened = gui.eventually(ready, "Action-close instance not ready")
        window, editor = reopened["window"], reopened["editor"]
        clean()
        key(0x4E, 1)
        send(window, 0x10, 0, 0)
        process.wait(timeout=8)
        assert process.returncode == 0, "Close during a structural operation was lost"
        assert original_path.read_text(encoding="utf-8") == latest_close_text
        return {"passed": True, "desktop": desktop, "injected_save_delay_ms": DELAY_MS,
                "response_bound_ms": RESPONSE_BOUND_MS, "timings": timings,
                "checks": ["input and snapshot during slow save", "stale acknowledgement keeps newer dirty text",
                           "latest generation saved", "new and switch wait for save", "conflict retains external file",
                           "input after conflict and latest recovery", "failed transition retains buffer",
                           "pending operations save newer input", "close waits for IME end and latest save",
                           "close during a structural operation is retained"],
                "measurement_scope": "Injected test delay only; no product latency guarantee"}
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
        run(__file__, "native-async-ui-", timeout=60000)
