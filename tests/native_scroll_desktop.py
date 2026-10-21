"""Native wheel, scrollbar and modal isolation checks on a private desktop.

Only messages to owned HWNDs are used; no global mouse, keyboard or clipboard.
MINIMAL_EDITOR_NATIVE_EXE can select an isolated candidate executable.
"""
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import subprocess
import struct
import sys
import traceback
import zlib
import gui_desktop as gui
from private_desktop import run, verify_desktop, screenshot

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])
move = gui.bind(gui.U, "MoveWindow", W.BOOL, [W.HWND, C.c_int, C.c_int, C.c_int, C.c_int, W.BOOL])
get_client = gui.bind(gui.U, "GetClientRect", W.BOOL, [W.HWND, C.c_void_p])
get_window = gui.bind(gui.U, "GetWindowRect", W.BOOL, [W.HWND, C.c_void_p])
client_to_screen = gui.bind(gui.U, "ClientToScreen", W.BOOL, [W.HWND, C.c_void_p])


class Point(C.Structure):
    _fields_ = [("x", C.c_long), ("y", C.c_long)]


child_at = gui.bind(gui.U, "ChildWindowFromPointEx", W.HWND, [W.HWND, Point, W.UINT])
set_dpi = gui.bind(gui.U, "SetThreadDpiAwarenessContext", W.HANDLE, [W.HANDLE])


def packed(x, y):
    return (int(x) & 0xffff) | ((int(y) & 0xffff) << 16)


def child(folder, desktop):
    verify_desktop(desktop)
    gui.checked(set_dpi(C.c_void_p(-4)))
    exe = Path(os.environ.get("MINIMAL_EDITOR_NATIVE_EXE", str(gui.ROOT / "build/native-next.exe")))
    data, report = folder / "data", folder / "state.json"
    (data / "memos").mkdir(parents=True)
    texts = {"short": "短い本文。", "long": (("日本語の長い文章を書く。Latin 123。" * 8 + "\n") * 700), "wrap": "あ" * 300}
    lines = ["minimal-editor-native-v1", "active\tshort"]
    for i, memo_id in enumerate(["short", "long", "wrap"] + [f"extra{j:02}" for j in range(37)]):
        title = f"メモ {memo_id}"
        lines.append(f"memo\t{memo_id}\t{1000 + i}\t0\t{title.encode('utf-8').hex()}")
        (data / "memos" / f"{memo_id}.txt").write_text(texts.get(memo_id, title), encoding="utf-8")
    (data / "catalog.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8")
    result = {"passed": False, "desktop": desktop, "executable": str(exe), "observations": {}, "images": []}
    process = None

    def state():
        send(window, 0x8002, 0, 0)
        return json.loads(report.read_text(encoding="utf-8"))

    def until(predicate, message):
        def check():
            value = state()
            return value if predicate(value) else None
        return gui.eventually(check, message)

    def key(vk, mods=0):
        send(window, 0x8001, vk, mods)

    def compact(value):
        return {k: value[k] for k in ("active", "bodyVisible", "bodyScroll", "bodySelection", "scrollMax", "thumbVisible", "thumbRect")}

    def wheel(hwnd, delta=-120, point=None):
        if point is None:
            rect = (C.c_long * 4)()
            gui.checked(get_client(hwnd, rect))
            point = Point((rect[2] - rect[0]) // 2, (rect[3] - rect[1]) // 2)
        else:
            point = Point(*point)
        gui.checked(client_to_screen(hwnd, C.byref(point)))
        send(hwnd, 0x20A, (delta & 0xffff) << 16, packed(point.x, point.y))

    def open_memo(memo_id):
        key(0x50, 1)
        picker = state()
        assert picker["picker"] == "normal"
        at = next(i for i, memo in enumerate(picker["items"]) if memo["id"] == memo_id)
        for _ in range(picker["selected"] - at):
            key(0x26)
        for _ in range(at - picker["selected"]):
            key(0x28)
        key(0x0D)
        return until(lambda s: s["active"] == memo_id and not s["busy"] and s["picker"] is None,
                     f"Memo {memo_id} did not open")

    def picture(name):
        path = folder / f"{name}.png"
        screenshot(window, process.pid, path)
        result["images"].append(str(path))
        if name in ("long-scrolled", "narrow-wrap-scroll"):
            # Inspect actual rendered pixels, including the area outside the
            # formatting rectangle where RichEdit used to leave stale text.
            png = path.read_bytes()
            width, height = struct.unpack_from(">II", png, 16)
            offset, compressed = 8, b""
            while offset < len(png):
                length = struct.unpack_from(">I", png, offset)[0]
                if png[offset + 4:offset + 8] == b"IDAT":
                    compressed += png[offset + 8:offset + 8 + length]
                offset += length + 12
            pixels = zlib.decompress(compressed)
            stride = width * 3 + 1  # private_desktop.screenshot writes filter 0 RGB
            assert len(pixels) == stride * height
            frame, client = (C.c_long * 4)(), (C.c_long * 4)()
            gui.checked(get_window(window, frame))
            gui.checked(get_client(window, client))
            origin = Point(0, 0)
            gui.checked(client_to_screen(window, C.byref(origin)))
            dx, dy = origin.x - frame[0], origin.y - frame[1]
            current = state()
            body_origin = Point(0, 0)
            gui.checked(client_to_screen(editor, C.byref(body_origin)))
            left = body_origin.x - frame[0] + current["formatRect"][0]
            right = body_origin.x - frame[0] + current["formatRect"][2]
            top = dy + client[3] - round(48 * current["scale"]) + 4
            bottom = dy + client[3] - 4
            count = sum(pixels[y * stride + 1 + x * 3:y * stride + 4 + x * 3] != b"\x20\x20\x20"
                        for y in range(top, bottom) for x in range(left, right))
            result["observations"][name + "_bottom_margin_ink"] = count
            assert count == 0, f"Stale text in bottom margin: {count} pixels ({path})"
            top_count = sum(pixels[y * stride + 1 + x * 3:y * stride + 4 + x * 3] != b"\x20\x20\x20"
                            for y in range(dy + 4, body_origin.y - frame[1] - 4)
                            for x in range(left, right))
            result["observations"][name + "_top_margin_ink"] = top_count
            assert top_count == 0, f"Stale text in top margin: {top_count} pixels"

    try:
        with (folder / "stdout.txt").open("wb") as out, (folder / "stderr.txt").open("wb") as err:
            process = subprocess.Popen([str(exe), "--test-mode", "--data-dir", str(data), "--report-file", str(report)],
                                       stdout=out, stderr=err, creationflags=subprocess.CREATE_NO_WINDOW)

        def ready():
            assert process.poll() is None, "Native exited before ready"
            owned = next((w for w in gui.windows() if w["pid"] == process.pid and
                          w["class"].startswith("MinimalEditorNext-") and w["visible"]), None)
            if owned:
                send(owned["hwnd"], 0x8002, 0, 0)
                if report.exists():
                    value = json.loads(report.read_text(encoding="utf-8"))
                    return value if value.get("ready") else None
        first = gui.eventually(ready, "Native did not load")
        window, editor, overlay, thumb = first["window"], first["editor"], first["overlay"], first["thumb"]
        needed = {"bodyScroll", "bodySelection", "bodyVisible", "scrollMax", "thumbVisible", "thumbRect"}
        assert needed <= first.keys(), f"Missing instrumentation: {sorted(needed - first.keys())}"
        gui.checked(move(window, 0, 0, 1100, 800, True))
        wheel(editor)
        short = state()
        result["observations"]["short_after_wheel"] = compact(short)
        assert short["active"] == "short" and short["scrollMax"] == 0 and not short["thumbVisible"], compact(short)
        assert short["bodyScroll"][1] == 0, compact(short)

        long = open_memo("long")
        assert long["scrollMax"] > 0 and long["bodyVisible"], compact(long)
        # Compare the real RichEdit formatting area and thumb in host client
        # coordinates. Wide windows previously left the thumb inside the text.
        gaps = []
        for width in (1700, 460, 1100, 2200):
            gui.checked(move(window, 0, 0, width, 800, True))
            wheel(editor)
            geometry = state()
            origin, host = Point(0, 0), Point(0, 0)
            gui.checked(client_to_screen(editor, C.byref(origin)))
            gui.checked(client_to_screen(window, C.byref(host)))
            client, viewport = (C.c_long * 4)(), (C.c_long * 4)()
            gui.checked(get_client(window, client))
            gui.checked(get_client(editor, viewport))
            expected_top = round((36 if client[2] <= round(600 * geometry["scale"]) else 60) * geometry["scale"])
            assert origin.y - host.y == expected_top
            assert client[3] - (origin.y - host.y + viewport[3]) == round(48 * geometry["scale"])
            text_right = origin.x - host.x + geometry["formatRect"][2]
            gap = geometry["thumbRect"][0] - text_right
            gaps.append({"windowWidth": width, "textRight": text_right,
                         "thumbLeft": geometry["thumbRect"][0], "gap": gap})
            result["observations"]["text_thumb_gaps"] = gaps
            assert geometry["thumbVisible"] and gap >= int(12 * geometry["scale"]) - 1, gaps
            if width == 1700:
                picture("wide-text-scroll-gap")
        gui.checked(move(window, 0, 0, 1100, 800, True))
        long = state()
        before = long["bodyScroll"][1]
        wheel(editor)
        wheeled = until(lambda s: s["bodyScroll"][1] > before, "Long body did not scroll on real WM_MOUSEWHEEL")
        result["observations"]["long_after_wheel"] = compact(wheeled)
        assert wheeled["thumbVisible"], compact(wheeled)
        # The host receives wheel events too (for example over the custom
        # scrollbar); its forwarding must scroll the same body.
        wheel(window)
        host_wheel = state()
        assert host_wheel["bodyScroll"][1] > wheeled["bodyScroll"][1], compact(host_wheel)
        # Preserve sub-notch wheel input rather than dropping high-resolution
        # deltas. Two half notches must equal one complete downward gesture.
        wheel(editor, delta=-60)
        half = state()
        assert half["bodyScroll"] == host_wheel["bodyScroll"], compact(half)
        wheel(editor, delta=-60)
        wheeled = state()
        assert wheeled["bodyScroll"][1] > half["bodyScroll"][1], compact(wheeled)
        rect = wheeled["thumbRect"]
        center = Point((rect[0] + rect[2]) // 2, (rect[1] + rect[3]) // 2)
        hit = child_at(window, center, 0x1 | 0x2 | 0x4)
        result["observations"]["thumb_hit_hwnd"] = int(hit or 0)
        assert hit == thumb, f"Scrollbar was covered: hit {hit}, expected {thumb}"
        local_x, local_y = max(1, (rect[2] - rect[0]) // 2), max(1, (rect[3] - rect[1]) // 2)
        send(thumb, 0x201, 1, packed(local_x, local_y))
        try:
            send(thumb, 0x200, 1, packed(local_x, local_y + 150))
        finally:
            send(thumb, 0x202, 0, packed(local_x, local_y + 150))
        dragged = state()
        result["observations"]["long_after_drag"] = compact(dragged)
        assert dragged["bodyScroll"][1] > wheeled["bodyScroll"][1], compact(dragged)
        # Canceling a captured drag must not leave a stale drag origin that
        # makes later pointer motion scroll the document without a button.
        send(thumb, 0x201, 1, packed(local_x, local_y))
        send(thumb, 0x1F, 0, 0)
        canceled = state()
        send(thumb, 0x200, 0, packed(local_x, local_y + 100))
        assert state()["bodyScroll"] == canceled["bodyScroll"], "Canceled drag continued on hover"
        picture("long-scrolled")

        # Setting a selection and then scrolling supplies non-default state
        # which opening, navigating and closing the modal must preserve.
        send(editor, 0xB1, 20, 50)
        wheel(editor)
        preserved = state()
        key(0x50, 1)
        modal = state()
        assert not modal["bodyVisible"] and not modal["thumbVisible"], compact(modal)
        modal_before = modal["bodyScroll"]
        rect = modal["pickerRect"]
        wheel(overlay, point=((rect[0] + rect[2]) // 2, (rect[1] + rect[3]) // 2))
        modal_after = state()
        assert modal_after["first"] != modal["first"], "Picker did not scroll"
        assert modal_after["bodyScroll"] == modal_before and modal_after["bodySelection"] == preserved["bodySelection"], compact(modal_after)
        picture("modal-body-hidden")
        key(0x1B)
        closed = state()
        result["observations"]["before_modal"] = compact(preserved)
        result["observations"]["after_modal"] = compact(closed)
        assert closed["bodyVisible"] and closed["bodyScroll"] == preserved["bodyScroll"], (compact(preserved), compact(closed))
        assert closed["bodySelection"] == preserved["bodySelection"] and closed["text"] == texts["long"], compact(closed)

        short_again = open_memo("short")
        assert not short_again["thumbVisible"] and short_again["scrollMax"] == 0, compact(short_again)
        wrap = open_memo("wrap")
        wheel(editor)
        wide = state()
        assert wide["scrollMax"] == 0 and not wide["thumbVisible"], compact(wide)
        gui.checked(move(window, 0, 0, 460, 620, True))
        narrow_before = state()
        assert narrow_before["scrollMax"] > 0, compact(narrow_before)
        wheel(editor)
        narrow = until(lambda s: s["bodyScroll"][1] > 0 and s["thumbVisible"], "Wrapped narrow body did not scroll/reveal scrollbar")
        picture("narrow-wrap-scroll")
        gui.checked(move(window, 0, 0, 1100, 800, True))
        wheel(editor)
        wide_again = state()
        assert wide_again["scrollMax"] == 0 and not wide_again["thumbVisible"], compact(wide_again)
        result["observations"].update({"wrap_wide": compact(wide), "wrap_narrow": compact(narrow), "wrap_wide_again": compact(wide_again)})
        assert not wide_again["dirty"] and not wide_again["error"]
        result["passed"] = True
        result["checks"] = ["blank top and bottom margins after scrolling", "text and thumb separated at narrow and wide widths", "short text scrollbar hidden", "real wheel scrolls long text", "thumb hit testing",
                            "thumb drag scrolls", "modal hides body and scrollbar", "picker wheel leaves body unchanged",
                            "modal preserves selection and scroll", "short memo hides scrollbar", "resize wrapping updates scroll need"]
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
        run(__file__, "native-scroll-ui-", timeout=60000)
