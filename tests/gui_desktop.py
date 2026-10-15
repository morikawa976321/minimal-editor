"""Exercise normal native GUI launches on a private, never-displayed desktop.

Run with the existing Python interpreter after building build/native.exe.
No SwitchDesktop, keyboard, clipboard, or user-window operations are used.
The parent owns a kill-on-close job for every test descendant.
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

ROOT = Path(__file__).resolve().parents[1]
K = C.WinDLL("kernel32", use_last_error=True)
U = C.WinDLL("user32", use_last_error=True)


class StartupInfo(C.Structure):
    _fields_ = [("cb", W.DWORD), ("reserved", W.LPWSTR),
                ("desktop", W.LPWSTR), ("title", W.LPWSTR),
                ("x", W.DWORD), ("y", W.DWORD), ("width", W.DWORD),
                ("height", W.DWORD), ("chars_x", W.DWORD),
                ("chars_y", W.DWORD), ("fill", W.DWORD),
                ("flags", W.DWORD), ("show", W.WORD),
                ("reserved_size", W.WORD), ("reserved_data", C.c_void_p),
                ("stdin", W.HANDLE), ("stdout", W.HANDLE), ("stderr", W.HANDLE)]


class ProcessInfo(C.Structure):
    _fields_ = [("process", W.HANDLE), ("thread", W.HANDLE),
                ("pid", W.DWORD), ("tid", W.DWORD)]


class BasicLimits(C.Structure):
    _fields_ = [("process_time", C.c_int64), ("job_time", C.c_int64),
                ("flags", W.DWORD), ("min_working", C.c_size_t),
                ("max_working", C.c_size_t), ("active_processes", W.DWORD),
                ("affinity", C.c_size_t), ("priority", W.DWORD),
                ("scheduling", W.DWORD)]


class ExtendedLimits(C.Structure):
    _fields_ = [("basic", BasicLimits), ("io", C.c_uint64 * 6),
                ("process_memory", C.c_size_t), ("job_memory", C.c_size_t),
                ("peak_process_memory", C.c_size_t), ("peak_job_memory", C.c_size_t)]


CALLBACK = C.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)


def bind(dll, name, result, args):
    fn = getattr(dll, name)
    fn.restype, fn.argtypes = result, args
    return fn


create_desktop = bind(U, "CreateDesktopW", W.HANDLE,
                      [W.LPCWSTR, C.c_void_p, C.c_void_p, W.DWORD, W.DWORD, C.c_void_p])
close_desktop = bind(U, "CloseDesktop", W.BOOL, [W.HANDLE])
get_thread_desktop = bind(U, "GetThreadDesktop", W.HANDLE, [W.DWORD])
get_thread_id = bind(K, "GetCurrentThreadId", W.DWORD, [])
get_object_info = bind(U, "GetUserObjectInformationW", W.BOOL,
                       [W.HANDLE, C.c_int, C.c_void_p, W.DWORD, C.POINTER(W.DWORD)])
enum_windows = bind(U, "EnumWindows", W.BOOL, [CALLBACK, W.LPARAM])
get_class = bind(U, "GetClassNameW", C.c_int, [W.HWND, W.LPWSTR, C.c_int])
get_pid = bind(U, "GetWindowThreadProcessId", W.DWORD, [W.HWND, C.POINTER(W.DWORD)])
visible = bind(U, "IsWindowVisible", W.BOOL, [W.HWND])
iconic = bind(U, "IsIconic", W.BOOL, [W.HWND])
show_window = bind(U, "ShowWindow", W.BOOL, [W.HWND, C.c_int])
post_message = bind(U, "PostMessageW", W.BOOL, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])
create_job = bind(K, "CreateJobObjectW", W.HANDLE, [C.c_void_p, W.LPCWSTR])
set_job = bind(K, "SetInformationJobObject", W.BOOL,
               [W.HANDLE, C.c_int, C.c_void_p, W.DWORD])
assign_job = bind(K, "AssignProcessToJobObject", W.BOOL, [W.HANDLE, W.HANDLE])
create_process = bind(K, "CreateProcessW", W.BOOL,
                      [W.LPCWSTR, W.LPWSTR, C.c_void_p, C.c_void_p, W.BOOL,
                       W.DWORD, C.c_void_p, W.LPCWSTR, C.POINTER(StartupInfo),
                       C.POINTER(ProcessInfo)])
resume = bind(K, "ResumeThread", W.DWORD, [W.HANDLE])
wait = bind(K, "WaitForSingleObject", W.DWORD, [W.HANDLE, W.DWORD])
get_exit = bind(K, "GetExitCodeProcess", W.BOOL, [W.HANDLE, C.POINTER(W.DWORD)])
terminate = bind(K, "TerminateProcess", W.BOOL, [W.HANDLE, W.UINT])
close = bind(K, "CloseHandle", W.BOOL, [W.HANDLE])


def checked(value):
    if not value:
        raise C.WinError(C.get_last_error())
    return value


def windows():
    found = []

    @CALLBACK
    def visit(hwnd, _):
        name = C.create_unicode_buffer(256)
        get_class(hwnd, name, len(name))
        pid = W.DWORD()
        get_pid(hwnd, C.byref(pid))
        found.append({"hwnd": hwnd, "pid": pid.value, "class": name.value,
                      "visible": bool(visible(hwnd)), "minimized": bool(iconic(hwnd))})
        return True

    C.set_last_error(0)
    if not enum_windows(visit, 0) and C.get_last_error():
        raise C.WinError(C.get_last_error())
    return found


def editors():
    all_windows = windows()
    assert not any(w["class"] == "#32770" for w in all_windows), all_windows
    return [w for w in all_windows if w["class"].startswith("MinimalEditorNativeV0-")]


def eventually(predicate, message, seconds=8):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        result = predicate()
        if result:
            return result
        time.sleep(0.04)
    raise AssertionError(message + ": " + repr(windows()))


def child(folder, expected_desktop):
    # This interpreter and all descendants were created on the private desktop.
    # EnumWindows therefore cannot address the user's desktop windows.
    desktop_name = C.create_unicode_buffer(256)
    size = W.DWORD()
    checked(get_object_info(get_thread_desktop(get_thread_id()), 2,
                            desktop_name, C.sizeof(desktop_name), C.byref(size)))
    assert desktop_name.value == expected_desktop, "Refusing GUI tests outside the owned desktop"
    node = shutil.which("node")
    assert node, "Node not found"
    native = ROOT / "build/native.exe"
    assert native.is_file(), "Build native.exe first"
    owned = set()
    results = []

    def launch(data, legacy=False):
        args = json.dumps(["--data-dir", str(data)])
        if legacy:
            source = ("import {spawn} from 'node:child_process';"
                      f"const child=spawn({json.dumps(str(native))},{args},"
                      "{detached:true,stdio:'ignore',windowsHide:true});"
                      "child.once('error',e=>{console.error(e);process.exit(1)});"
                      "child.once('spawn',()=>child.unref());")
        else:
            source = (f"import {{startProcess}} from {json.dumps((ROOT / 'scripts/server.mjs').as_uri())};"
                      f"await startProcess({json.dumps(str(native))},{args});")
        subprocess.run([node, "--input-type=module", "-e", source], cwd=ROOT,
                       creationflags=subprocess.CREATE_NO_WINDOW,
                       capture_output=True, text=True, timeout=10, check=True)

    try:
        assert not editors(), "Private desktop unexpectedly contains an editor"
        for legacy in (False, True):
            label = "legacy_windowsHide_true" if legacy else "production_startProcess"
            data = folder / label
            launch(data, legacy)
            first = eventually(lambda: (e[0] if len(e := editors()) == 1 and e[0]["visible"] else None),
                               label + " failed to show its normal GUI")
            owned.add(first["hwnd"])
            show_window(first["hwnd"], 6)  # SW_MINIMIZE, on private desktop only.
            eventually(lambda: iconic(first["hwnd"]), "Failed to minimize test window")
            launch(data, legacy)
            eventually(lambda: visible(first["hwnd"]) and not iconic(first["hwnd"]),
                       "Second launch did not restore existing editor")
            # Observe for delayed error dialogs or duplicate editor windows.
            until = time.monotonic() + 1
            while time.monotonic() < until:
                current = editors()
                assert len(current) == 1 and current[0]["hwnd"] == first["hwnd"], current
                time.sleep(0.04)
            checked(post_message(first["hwnd"], 0x0010, 0, 0))
            eventually(lambda: not editors(), "Owned editor did not close")
            owned.remove(first["hwnd"])
            results.append({"launch": label, "visible": True,
                            "second_launch_restored_same_window": True,
                            "duplicate_windows": 0, "error_dialogs": 0})
        return {"passed": True, "desktop": desktop_name.value, "cases": results}
    finally:
        # The parent job also terminates any remaining test descendants.
        for hwnd in owned:
            post_message(hwnd, 0x0010, 0, 0)


def parent():
    (ROOT / ".work").mkdir(exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix="gui-desktop-", dir=ROOT / ".work"))
    name = "MinimalEditorTest_" + uuid.uuid4().hex
    desktop = job = None
    pi = ProcessInfo()
    assigned = False
    try:
        # No DESKTOP_SWITCHDESKTOP access and no SwitchDesktop call anywhere.
        desktop = checked(create_desktop(name, None, None, 0, 0x00BF, None))
        job = checked(create_job(None, None))
        limits = ExtendedLimits()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        checked(set_job(job, 9, C.byref(limits), C.sizeof(limits)))
        si = StartupInfo()
        si.cb, si.desktop = C.sizeof(si), name
        command = C.create_unicode_buffer(subprocess.list2cmdline(
            [sys.executable, str(Path(__file__).resolve()), "--child", str(folder), name]))
        checked(create_process(sys.executable, command, None, None, False,
                               0x08000004, None, str(ROOT), C.byref(si), C.byref(pi)))
        checked(assign_job(job, pi.process))
        assigned = True
        if resume(pi.thread) == 0xFFFFFFFF:
            raise C.WinError(C.get_last_error())
        assert wait(pi.process, 45000) == 0, "Private-desktop GUI test timed out"
        code = W.DWORD()
        checked(get_exit(pi.process, C.byref(code)))
        report = folder / "result.json"
        result = json.loads(report.read_text(encoding="utf-8")) if report.exists() else {"exit_code": code.value}
        print(json.dumps({**result, "report": str(report)}, ensure_ascii=False))
        assert code.value == 0 and result.get("passed"), result
    finally:
        if pi.process and not assigned:
            terminate(pi.process, 1)  # Only the child created above, still suspended.
        if job:
            close(job)  # Ends only processes assigned to this private test job.
        if pi.process:
            wait(pi.process, 5000)
            close(pi.process)
        if pi.thread:
            close(pi.thread)
        if desktop:
            checked(close_desktop(desktop))


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
