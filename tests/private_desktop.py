"""Shared runner for private GUI tests; never switches the displayed desktop."""
import ctypes as C
from ctypes import wintypes as W
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import uuid
import zlib
import gui_desktop as gui


def verify_desktop(expected):
    name = C.create_unicode_buffer(256)
    length = W.DWORD()
    gui.checked(gui.get_object_info(gui.get_thread_desktop(gui.get_thread_id()), 2,
                                   name, C.sizeof(name), C.byref(length)))
    assert name.value == expected, "Refusing test outside owned desktop"


def run(script, prefix, timeout=60000):
    (gui.ROOT / ".work").mkdir(exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix=prefix, dir=gui.ROOT / ".work"))
    name = "MinimalEditorPrivate_" + uuid.uuid4().hex
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
            [sys.executable, str(Path(script).resolve()), "--child", str(folder), name]))
        gui.checked(gui.create_process(sys.executable, command, None, None, False,
                                       0x08000004, None, str(gui.ROOT), C.byref(si), C.byref(pi)))
        gui.checked(gui.assign_job(job, pi.process))
        assigned = True
        assert gui.resume(pi.thread) != 0xFFFFFFFF
        # Bounded native wait; test scripts write their own result and exit.
        assert gui.wait(pi.process, timeout) == 0, "Private desktop test timed out"
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


def screenshot(hwnd, pid, path):
    """Print only an owned window into a PNG; never captures the user desktop."""
    owner = W.DWORD()
    gui.get_pid(hwnd, C.byref(owner))
    assert owner.value == pid
    # GetWindowRect is DPI-virtualized for the Python harness by default; match
    # the app's per-monitor awareness so the captured DIB is not cropped.
    set_dpi = gui.bind(gui.U, "SetThreadDpiAwarenessContext", W.HANDLE, [W.HANDLE])
    previous_dpi = gui.checked(set_dpi(C.c_void_p(-4)))
    get_rect = gui.bind(gui.U, "GetWindowRect", W.BOOL, [W.HWND, C.c_void_p])
    get_dc = gui.bind(gui.U, "GetDC", W.HDC, [W.HWND])
    release_dc = gui.bind(gui.U, "ReleaseDC", C.c_int, [W.HWND, W.HDC])
    print_window = gui.bind(gui.U, "PrintWindow", W.BOOL, [W.HWND, W.HDC, W.UINT])
    gdi = C.WinDLL("gdi32", use_last_error=True)
    create_dc = gui.bind(gdi, "CreateCompatibleDC", W.HDC, [W.HDC])
    create_dib = gui.bind(gdi, "CreateDIBSection", W.HBITMAP,
                          [W.HDC, C.c_void_p, W.UINT, C.POINTER(C.c_void_p), W.HANDLE, W.DWORD])
    select = gui.bind(gdi, "SelectObject", W.HANDLE, [W.HDC, W.HANDLE])
    delete = gui.bind(gdi, "DeleteObject", W.BOOL, [W.HANDLE])
    delete_dc = gui.bind(gdi, "DeleteDC", W.BOOL, [W.HDC])
    rect = (C.c_long * 4)()
    gui.checked(get_rect(hwnd, rect))
    width, height = rect[2] - rect[0], rect[3] - rect[1]
    assert 0 < width < 10000 and 0 < height < 10000
    info = C.create_string_buffer(struct.pack("<IiiHHIIiiII", 40, width, -height, 1, 32, 0, width * height * 4, 0, 0, 0, 0))
    dc = gui.checked(get_dc(hwnd))
    memory_dc = gui.checked(create_dc(dc))
    bits = C.c_void_p()
    bitmap = gui.checked(create_dib(dc, info, 0, C.byref(bits), None, 0))
    old = select(memory_dc, bitmap)
    try:
        gui.checked(print_window(hwnd, memory_dc, 2))
        bgra = C.string_at(bits, width * height * 4)
        rows = []
        for y in range(height):
            row = bgra[y * width * 4:(y + 1) * width * 4]
            rgb = bytearray(width * 3)
            rgb[0::3], rgb[1::3], rgb[2::3] = row[2::4], row[1::4], row[0::4]
            rows.append(b"\0" + rgb)
        def chunk(kind, data):
            return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
        path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
                         + chunk(b"IDAT", zlib.compress(b"".join(rows))) + chunk(b"IEND", b""))
    finally:
        select(memory_dc, old)
        delete(bitmap)
        delete_dc(memory_dc)
        release_dc(hwnd, dc)
        set_dpi(previous_dpi)
