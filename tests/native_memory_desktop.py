"""Compare GUI architecture probes and the actual native app; no browser needed."""
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
from memory_desktop import sample, SystemMemory, system_memory

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])
find_child = gui.bind(gui.U, "FindWindowExW", W.HWND, [W.HWND, W.HWND, W.LPCWSTR, W.LPCWSTR])


def child(folder, desktop):
    verify_desktop(desktop)
    available = SystemMemory()
    available.length = C.sizeof(available)
    gui.checked(system_memory(C.byref(available)))
    text = "日本語の文章を書く。Latin 123。\n" * 700
    cases = []
    for kind in ("bare", "edit", "richedit", "console-worker", "native-full"):
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
                    for _ in range(40):
                        value = C.create_unicode_buffer("追記。Latin。\r\n")
                        started = time.perf_counter()
                        send(editor, 0xC2, 1, C.cast(value, C.c_void_p).value)
                        latencies.append((time.perf_counter() - started) * 1000)
                    assert (data / "memos/initial.txt").read_text(encoding="utf-8") == text + "追記。Latin。\n" * 40
                    points = []
                    for _ in range(3):
                        points.append(sample(process.pid))
                        time.sleep(0.15)
                    cases.append({"kind": kind, "state": "after40edits", "trial": trial,
                                  "text_characters": len(text) + 40 * len("追記。Latin。\n"), "samples": points,
                                  "insert_and_sync_ms": latencies})
            finally:
                if process.poll() is None:
                    process.terminate()
                process.wait(timeout=5)
    return {"passed": True, "free_ram_bytes_at_start": available.free, "cases": cases,
            "notes": ["Single process values; Python test runner excluded.",
                      "Native app measured without test-mode/report snapshots, comparison helper, or browser.",
                      "Probe modes are incomplete controls, not full editors.",
                      "console-worker has no terminal, editing or rendering; it is not TUI memory.",
                      "Private desktop, no real IME/Handy or working-set trimming; sampled states are not peak bounds."]}


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
        run(__file__, "native-memory-")
