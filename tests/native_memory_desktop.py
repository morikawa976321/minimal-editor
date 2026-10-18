"""Compare GUI architecture probes and the actual native app; no browser needed."""
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
from private_desktop import run, verify_desktop
from memory_desktop import sample, SystemMemory, system_memory

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])
find_child = gui.bind(gui.U, "FindWindowExW", W.HWND, [W.HWND, W.HWND, W.LPCWSTR, W.LPCWSTR])


def child(folder, desktop, native_only=False):
    verify_desktop(desktop)
    available = SystemMemory()
    available.length = C.sizeof(available)
    gui.checked(system_memory(C.byref(available)))
    text = "日本語の文章を書く。Latin 123。\n" * 700
    cases = []
    kinds = ("native-full",) if native_only else ("bare", "edit", "richedit", "console-worker", "native-full")
    for kind in kinds:
        for trial in range(1, 4):
            data = folder / f"{kind}-{trial}"
            data.mkdir()
            if kind == "native-full":
                (data / "memos").mkdir()
                (data / "memos/initial.txt").write_text(text, encoding="utf-8")
                (data / "catalog.tsv").write_text("minimal-editor-native-v1\nactive\tinitial\nmemo\tinitial\t1\t0\t-\n", encoding="utf-8")
                command = [str(gui.ROOT / "build/native-next.exe"), "--data-dir", str(data)]
            else:
                (data / "probe.txt").write_text(text, encoding="utf-8")
                command = [str(gui.ROOT / "build/memory-probe.exe"), "--mode", kind, "--hold-ms", "15000", "--data-dir", str(data)]
            with (data / "stdout.txt").open("wb") as out, (data / "stderr.txt").open("wb") as err:
                process = subprocess.Popen(command, stdout=out, stderr=err, creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                if kind == "native-full":
                    def ready():
                        assert process.poll() is None, (data / "stderr.txt").read_text(errors="replace")
                        return next((w for w in gui.windows() if w["pid"] == process.pid and w["class"].startswith("MinimalEditorNext-") and w["visible"]), None)
                    window = gui.eventually(ready, "Full native window did not load")["hwnd"]
                else:
                    gui.eventually(lambda: (data / "ready.json").exists(), "Architecture probe did not load")
                time.sleep(0.25)
                points = []
                for _ in range(3):
                    points.append(sample(process.pid))
                    time.sleep(0.15)
                cases.append({"kind": kind, "state": "idle", "trial": trial, "text_characters": len(text), "samples": points})
                if kind == "native-full":
                    editor = gui.checked(find_child(window, None, "RICHEDIT50W", None))
                    # Actual RichEdit path and durable writes, with bounded native Undo history.
                    send(editor, 0xB1, -1 & ((1 << (C.sizeof(C.c_size_t) * 8)) - 1), -1)
                    latencies = []
                    batch_started = time.perf_counter()
                    for _ in range(40):
                        value = C.create_unicode_buffer("追記。Latin。\r\n")
                        started = time.perf_counter()
                        send(editor, 0xC2, 1, C.cast(value, C.c_void_p).value)
                        latencies.append((time.perf_counter() - started) * 1000)
                    dispatched = time.perf_counter()
                    expected = text + "追記。Latin。\n" * 40
                    def saved_latest():
                        try:
                            return (data / "memos/initial.txt").read_text(encoding="utf-8") == expected
                        except PermissionError:
                            return False
                    gui.eventually(saved_latest, "Latest asynchronous native save did not finish")
                    flushed = time.perf_counter()
                    points = []
                    for _ in range(3):
                        points.append(sample(process.pid))
                        time.sleep(0.15)
                    cases.append({"kind": kind, "state": "after40edits", "trial": trial,
                                  "text_characters": len(text) + 40 * len("追記。Latin。\n"), "samples": points,
                                  "insert_dispatch_ms": latencies,
                                  "all_edits_saved_ms": (flushed - batch_started) * 1000,
                                  "after_last_insert_until_saved_ms": (flushed - dispatched) * 1000})
            finally:
                if process.poll() is None:
                    process.terminate()
                process.wait(timeout=5)
    return {"passed": True, "native_only": native_only, "free_ram_bytes_at_start": available.free, "cases": cases,
            "notes": ["Single process values; Python test runner excluded.",
                      "Native app measured without test-mode/report snapshots, comparison helper, or browser.",
                      "40 edits dispatch without waiting for individual saves; memory sampled after latest file contents match.",
                      "insert_dispatch_ms excludes background I/O; all_edits_saved_ms runs from first dispatch to latest-file verification, including polling delay.",
                      "Probe modes are incomplete controls, not full editors.",
                      "console-worker has no terminal, editing or rendering; it is not TUI memory.",
                      "Private desktop, no real IME/Handy or working-set trimming; sampled states are not peak bounds."]}


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        folder = Path(sys.argv[2])
        try:
            result = child(folder, sys.argv[3], os.environ.get("MINIMAL_EDITOR_MEMORY_NATIVE_ONLY") == "1")
        except BaseException:
            result = {"passed": False, "error": traceback.format_exc()}
        (folder / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        sys.exit(0 if result["passed"] else 1)
    else:
        if "--native-only" in sys.argv:
            # Process-local setting inherited by the private-desktop child;
            # no user or machine environment is changed.
            os.environ["MINIMAL_EDITOR_MEMORY_NATIVE_ONLY"] = "1"
        run(__file__, "native-memory-")
