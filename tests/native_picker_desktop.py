"""Picker paint regression checks on an owned, never-displayed desktop.

Set MINIMAL_EDITOR_PICKER_EXE to an instrumented baseline executable to record
the old behavior. GDI tolerance is a test heuristic, not a product guarantee.
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
from private_desktop import run, verify_desktop, screenshot

send = gui.bind(gui.U, "SendMessageW", C.c_ssize_t, [W.HWND, W.UINT, W.WPARAM, W.LPARAM])
redraw = gui.bind(gui.U, "RedrawWindow", W.BOOL, [W.HWND, C.c_void_p, W.HANDLE, W.UINT])
move = gui.bind(gui.U, "MoveWindow", W.BOOL, [W.HWND, C.c_int, C.c_int, C.c_int, C.c_int, W.BOOL])
style = gui.bind(gui.U, "GetWindowLongPtrW", C.c_ssize_t, [W.HWND, C.c_int])
get_dc = gui.bind(gui.U, "GetDC", W.HDC, [W.HWND])
release_dc = gui.bind(gui.U, "ReleaseDC", C.c_int, [W.HWND, W.HDC])
map_points = gui.bind(gui.U, "MapWindowPoints", C.c_int, [W.HWND, W.HWND, C.c_void_p, W.UINT])
gdi = C.WinDLL("gdi32", use_last_error=True)
clip_box = gui.bind(gdi, "GetClipBox", C.c_int, [W.HDC, C.c_void_p])
rect_visible = gui.bind(gdi, "RectVisible", W.BOOL, [W.HDC, C.c_void_p])
open_process = gui.bind(gui.K, "OpenProcess", W.HANDLE, [W.DWORD, W.BOOL, W.DWORD])
resources = gui.bind(gui.U, "GetGuiResources", W.DWORD, [W.HANDLE, W.DWORD])
COUNTERS = ("bodyPaints", "bodyLayoutCalls", "bodySetRectCalls", "pickerPaints",
            "pickerBufferedPresents", "pickerDirectFallbacks")
GDI_TOLERANCE = 2


def child(folder, desktop):
    verify_desktop(desktop)
    exe = Path(os.environ.get("MINIMAL_EDITOR_PICKER_EXE", str(gui.ROOT / "build/native-next.exe")))
    data, report = folder / "data", folder / "state.json"
    (data / "memos").mkdir(parents=True)
    body = ("自分の文章を書く。余白と道具。Latin 123。\n" * 600)[:15000]
    assert len(body) == 15000
    lines = ["minimal-editor-native-v1", "active\tnormal00"]
    for archived, prefix in ((False, "normal"), (True, "archived")):
        for i in range(40):
            memo_id = f"{prefix}{i:02}"
            title = f"{'保管' if archived else '通常'}のメモ {i:02}"
            lines.append(f"memo\t{memo_id}\t{1000 + i}\t{int(archived)}\t{title.encode('utf-8').hex()}")
            (data / "memos" / f"{memo_id}.txt").write_text(body if memo_id == "normal00" else title, encoding="utf-8")
    (data / "catalog.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8")
    result = {"passed": False, "desktop": desktop, "executable": str(exe), "navigation": [],
              "gdi_count_tolerance": GDI_TOLERANCE, "images": [], "checks_failed": []}
    process = handle = None

    def state():
        send(window, 0x8002, 0, 0)
        return json.loads(report.read_text(encoding="utf-8"))

    def paint():
        gui.checked(redraw(overlay, None, None, 0x101))  # invalidate + update now
        gui.checked(redraw(window, None, None, 0x180))  # flush pending child paints

    def key(vk, mods=0):
        send(window, 0x8001, vk, mods)

    def picture(name):
        path = folder / f"{name}.png"
        screenshot(window, process.pid, path)
        result["images"].append(str(path))

    def clipping(value):
        return {name: bool(style(value[name], -16) & 0x04000000) for name in ("editor", "overlay", "thumb")}

    def inspect_clip(value):
        dc = gui.checked(get_dc(editor))
        try:
            box = (C.c_long * 4)()
            r = value["pickerRect"]
            point = (C.c_long * 2)((r[0] + r[2]) // 2, (r[1] + r[3]) // 2)
            map_points(overlay, editor, point, 1)
            center = (C.c_long * 4)(point[0], point[1], point[0] + 1, point[1] + 1)
            return {"clip_box_kind": clip_box(dc, box), "clip_box": list(box),
                    "body_point_behind_picker_visible": bool(rect_visible(dc, center)),
                    "scope": "Body DC must be fully clipped beneath the opaque full-window picker"}
        finally:
            release_dc(editor, dc)

    try:
        with (folder / "stdout.txt").open("wb") as out, (folder / "stderr.txt").open("wb") as err:
            process = subprocess.Popen([str(exe), "--test-mode", "--data-dir", str(data), "--report-file", str(report)],
                                       stdout=out, stderr=err, creationflags=subprocess.CREATE_NO_WINDOW)
        handle = gui.checked(open_process(0x400, False, process.pid))

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
        window, editor, overlay = first["window"], first["editor"], first["overlay"]
        assert first["active"] == "normal00" and first["text"] == body, first
        missing = set(COUNTERS) - first.keys()
        assert not missing, f"Missing instrumentation: {sorted(missing)}"

        for mode, mods in (("normal", 1), ("archived", 3)):
            key(0x50, mods)
            paint()
            # Clear opening/layout paints before measuring navigation alone.
            time.sleep(0.08)
            paint()
            # RichEdit/GDI may initialize font caches when a new title first
            # appears. Visit every row before counting transient buffer leaks.
            for vk in [0x28] * 39 + [0x26] * 39:
                key(vk)
                paint()
            before = state()
            assert before["picker"] == mode and len(before["items"]) == 40, before
            assert not gui.visible(editor), "Modal must physically hide the body"
            # A parked pointer must not undo subsequent keyboard navigation.
            r, scale = before["pickerRect"], before["scale"]
            x = r[0] + round(40 * scale)
            y = r[1] + round(16 * scale) + (before["selected"] - before["first"]) * round(48 * scale) + round(24 * scale)
            point = (x & 0xffff) | ((y & 0xffff) << 16)
            send(overlay, 0x200, 0, point)
            hovered = state()
            key(0x28)
            keyboard_selected = state()["selected"]
            assert keyboard_selected != hovered["selected"], "Arrow did not move from hovered row"
            send(overlay, 0x200, 0, point)
            assert state()["selected"] == keyboard_selected, "Stationary pointer undid keyboard selection"
            paint()
            picture(f"{mode}-before")
            before = state()
            gdi_before = resources(handle, 0)
            first_positions = {before["first"]}
            selected_positions = {before["selected"]}
            # Reach both ends and cross the scroll boundary many times.
            for vk in [0x28] * 39 + [0x26] * 39 + [0x28] * 22:
                key(vk)
                paint()
                value = state()
                first_positions.add(value["first"])
                selected_positions.add(value["selected"])
                assert value["active"] == "normal00" and value["text"] == body, value
            after = state()
            gdi_after = resources(handle, 0)
            # A first traversal can retain a small fixed system font/DC cache.
            # Check another equal workload for continued per-frame growth.
            for vk in [0x26] * 39 + [0x28] * 39 + [0x26] * 22:
                key(vk)
                paint()
            gdi_confirmed = resources(handle, 0)
            delta = {name: after[name] - before[name] for name in COUNTERS}
            clip_before_repaint = inspect_clip(after)
            # Deliberately invalidate the covered editor after the navigation
            # counters were sampled, keeping this separate from that invariant.
            gui.checked(redraw(editor, None, None, 0x101))
            clip_after_repaint = inspect_clip(state())
            navigation = {"mode": mode, "keys": 100, "counter_delta": delta,
                          "selected_positions": sorted(selected_positions), "first_positions": sorted(first_positions),
                          "gdi_before": gdi_before, "gdi_after": gdi_after, "gdi_delta": gdi_after - gdi_before,
                          "gdi_after_confirmation_100_keys": gdi_confirmed,
                          "gdi_confirmation_delta": gdi_confirmed - gdi_after,
                          "clipping_styles": clipping(after), "clip_observation": clip_before_repaint,
                          "clip_after_forced_body_repaint": clip_after_repaint}
            result["navigation"].append(navigation)
            for name in ("bodyPaints", "bodyLayoutCalls", "bodySetRectCalls"):
                if delta[name] != 0:
                    result["checks_failed"].append(f"{mode}: {name} changed by {delta[name]} during navigation")
            if delta["pickerPaints"] < 100 or delta["pickerBufferedPresents"] < 100:
                result["checks_failed"].append(f"{mode}: paints were not buffered for every forced frame")
            if after["pickerDirectFallbacks"] != 0:
                result["checks_failed"].append(f"{mode}: direct painting fallback occurred")
            if not all(navigation["clipping_styles"].values()):
                result["checks_failed"].append(f"{mode}: missing WS_CLIPSIBLINGS")
            for label, clip in (("before", clip_before_repaint), ("after forced body repaint", clip_after_repaint)):
                if clip["clip_box_kind"] != 1 or clip["body_point_behind_picker_visible"]:
                    result["checks_failed"].append(f"{mode}: body was not fully clipped {label}")
            if len(first_positions) < 2 or len(selected_positions) < 40:
                result["checks_failed"].append(f"{mode}: selection did not cross scrolling boundaries")
            if abs(gdi_confirmed - gdi_after) > GDI_TOLERANCE:
                result["checks_failed"].append(f"{mode}: GDI count continued to change by {gdi_confirmed - gdi_after}")
            picture(f"{mode}-after")
            key(0x1B)
            assert state()["picker"] is None
            assert gui.visible(editor), "Closing modal did not restore body visibility"

        # Resizing may legitimately relayout. Verify clipping and ordinary
        # picker/title interactions survive both narrow and wide layouts.
        for width, height in ((460, 620), (1100, 800)):
            gui.checked(move(window, 0, 0, width, height, True))
            key(0x50, 1)
            paint()
            resized = state()
            assert resized["picker"] == "normal" and resized["text"] == body
            if not all(clipping(resized).values()):
                result["checks_failed"].append(f"resize {width}: missing WS_CLIPSIBLINGS")
            key(0x4D, 1)
            editing = state()
            assert editing["renaming"] and gui.visible(editing["titleEditor"]), editing
            picture(f"title-{width}")
            key(0x1B)
            key(0x50, 1)
            assert state()["picker"] == "normal"
            key(0x50, 3)
            assert state()["picker"] == "archived"
            key(0x50, 1)
            assert state()["picker"] == "normal"
            key(0x50, 1)
            assert state()["picker"] is None
        final = state()
        assert final["active"] == "normal00" and final["text"] == body
        assert not final["dirty"] and not final["error"], final
        result["passed"] = not result["checks_failed"]
        result["notes"] = ["Counters apply to synthetic navigation on an undisplayed private desktop.",
                           "No global keyboard, clipboard, displayed desktop, or real draft was used.",
                           "GDI +/-2 is a regression test tolerance, not an application memory guarantee.",
                           "Every row is warmed before GDI counts to exclude first-use font caches.",
                           "An additional 100-key workload checks continued GDI growth after the first measured traversal.",
                           "Forced paints and still screenshots do not constitute a human flicker perception test."]
    except BaseException:
        result["error"] = traceback.format_exc()
    finally:
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
        if handle:
            gui.close(handle)
    return result


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        folder = Path(sys.argv[2])
        result = child(folder, sys.argv[3])
        (folder / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        sys.exit(0 if result["passed"] else 1)
    else:
        run(__file__, "native-picker-ui-", timeout=60000)
