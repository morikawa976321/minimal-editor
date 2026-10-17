"""Measure existing executables on a private desktop, without opening personal drafts.

No working-set trimming, foreground activation, keyboard or clipboard access.
The browser is deliberately excluded when free RAM is below the repo's 6 GiB rule.
"""
import ctypes as C
from ctypes import wintypes as W
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import traceback
import uuid

import gui_desktop as gui


class Memory(C.Structure):
    _fields_ = [("cb", W.DWORD), ("faults", W.DWORD)] + [
        (name, C.c_size_t) for name in (
            "peak_working_set", "working_set", "peak_paged", "paged",
            "peak_nonpaged", "nonpaged", "pagefile", "peak_pagefile", "private_bytes",
            "private_working_set")] + [("shared_commit", C.c_ulonglong)]


class SystemMemory(C.Structure):
    _fields_ = [("length", W.DWORD), ("load", W.DWORD)] + [
        (name, C.c_ulonglong) for name in (
            "total", "free", "page", "freepage", "virtual", "freevirtual", "extended")]


open_process = gui.bind(gui.K, "OpenProcess", W.HANDLE, [W.DWORD, W.BOOL, W.DWORD])
get_memory = gui.bind(C.WinDLL("psapi", use_last_error=True), "GetProcessMemoryInfo",
                      W.BOOL, [W.HANDLE, C.POINTER(Memory), W.DWORD])
system_memory = gui.bind(gui.K, "GlobalMemoryStatusEx", W.BOOL, [C.POINTER(SystemMemory)])


def sample(pid):
    handle = gui.checked(open_process(0x0410, False, pid))
    try:
        memory = Memory()
        memory.cb = C.sizeof(memory)
        gui.checked(get_memory(handle, C.byref(memory), memory.cb))
        return {"working_set_bytes": memory.working_set, "private_commit_bytes": memory.private_bytes,
                "private_working_set_bytes": memory.private_working_set}
    finally:
        gui.close(handle)


def child(folder, desktop):
    name = C.create_unicode_buffer(256)
    size = W.DWORD()
    gui.checked(gui.get_object_info(gui.get_thread_desktop(gui.get_thread_id()), 2,
                                   name, C.sizeof(name), C.byref(size)))
    assert name.value == desktop, "Refusing launch outside owned desktop"
    available = SystemMemory()
    available.length = C.sizeof(available)
    gui.checked(system_memory(C.byref(available)))
    native = gui.ROOT / "build/native.exe"
    assert native.is_file(), "Build the existing native prototype first"
    node = shutil.which("node")
    assert node
    text = "日本語の文章を書く。Latin 123。\n" * 700
    cases = []
    for kind in ("native-empty", "native-text", "node-server"):
        for trial in range(1, 4):
            data = folder / f"{kind}-{trial}"
            data.mkdir()
            if kind.startswith("native"):
                (data / "native.txt").write_text(text if kind.endswith("text") else "", encoding="utf-8")
                command = [str(native), "--data-dir", str(data)]
            else:
                (data / "web.txt").write_text(text, encoding="utf-8")
                command = [node, str(gui.ROOT / "tests/web-fixture.mjs"), "43139", str(data)]
            with (data / "stdout.txt").open("wb") as out, (data / "stderr.txt").open("wb") as err:
                process = subprocess.Popen(command, cwd=gui.ROOT, stdout=out, stderr=err,
                                           creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                if kind.startswith("native"):
                    gui.eventually(lambda: any(w["pid"] == process.pid and w["visible"] for w in gui.editors()),
                                   "Native window not ready")
                else:
                    gui.eventually(lambda: "ready" in (data / "stdout.txt").read_text(), "Server not ready")
                measurements = []
                for _ in range(3):
                    time.sleep(0.5)
                    assert process.poll() is None
                    measurements.append(sample(process.pid))
                cases.append({"kind": kind, "trial": trial, "pid": process.pid,
                              "text_characters": len(text) if kind != "native-empty" else 0,
                              "samples": measurements})
            finally:
                if process.poll() is None:
                    process.terminate()
                process.wait(timeout=5)
    return {"passed": True, "desktop": desktop, "free_ram_bytes_at_start": available.free,
            "cases": cases, "notes": [
                "Working set includes shared resident pages; private commit is not resident RAM.",
                "Private working set is resident memory private to this process (PROCESS_MEMORY_COUNTERS_EX2).",
                "Existing native EDIT prototype lacks current web UI and has a known IME appearance complaint.",
                "Private desktop was never displayed; IME/Handy and active typing were not exercised.",
                "Browser excluded to limit memory use on the shared machine."]}


def parent():
    (gui.ROOT / ".work").mkdir(exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix="memory-desktop-", dir=gui.ROOT / ".work"))
    name = "MinimalEditorMemory_" + uuid.uuid4().hex
    desktop = job = None
    pi = gui.ProcessInfo()
    assigned = False
    try:
        desktop = gui.checked(gui.create_desktop(name, None, None, 0, 0x00BF, None))
        job = gui.checked(gui.create_job(None, None))
        limits = gui.ExtendedLimits()
        limits.basic.flags = 0x2000
        gui.checked(gui.set_job(job, 9, C.byref(limits), C.sizeof(limits)))
        si = gui.StartupInfo()
        si.cb, si.desktop = C.sizeof(si), name
        command = C.create_unicode_buffer(subprocess.list2cmdline(
            [sys.executable, str(Path(__file__).resolve()), "--child", str(folder), name]))
        gui.checked(gui.create_process(sys.executable, command, None, None, False,
                                       0x08000004, None, str(gui.ROOT), C.byref(si), C.byref(pi)))
        gui.checked(gui.assign_job(job, pi.process))
        assigned = True
        gui.resume(pi.thread)
        assert gui.wait(pi.process, 60000) == 0, "Memory benchmark timed out"
        code = W.DWORD()
        gui.checked(gui.get_exit(pi.process, C.byref(code)))
        report = folder / "result.json"
        result = json.loads(report.read_text(encoding="utf-8")) if report.exists() else {}
        print(json.dumps({**result, "report": str(report)}, ensure_ascii=False))
        assert code.value == 0 and result.get("passed"), result
    finally:
        if pi.process and not assigned:
            gui.terminate(pi.process, 1)
        if job:
            gui.close(job)
        if pi.process:
            gui.wait(pi.process, 5000)
            gui.close(pi.process)
        if pi.thread:
            gui.close(pi.thread)
        if desktop:
            gui.checked(gui.close_desktop(desktop))


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
        parent()
