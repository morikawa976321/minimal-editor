"""Test the real Windows app launcher on a private desktop, never displayed.

Only an isolated copy of the app and its dedicated profile are opened. The parent
owns a kill-on-close job containing every descendant. No SwitchDesktop exists.
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
import urllib.request
import gui_desktop as gui

ROOT = gui.ROOT


def child(folder, expected_desktop):
    desktop_name = C.create_unicode_buffer(256)
    size = W.DWORD()
    gui.checked(gui.get_object_info(gui.get_thread_desktop(gui.get_thread_id()), 2,
                                   desktop_name, C.sizeof(desktop_name), C.byref(size)))
    assert desktop_name.value == expected_desktop, "Refusing GUI test outside owned desktop"
    node = shutil.which("node")
    assert node, "Node not found"
    edge = Path("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe")
    assert edge.is_file(), "Existing Edge not found"
    isolated = folder / "app"
    for relative in ("scripts/browser-app.vbs", "scripts/launch-editor.ps1", "scripts/focus-editor.ps1",
                     "scripts/bootstrap.mjs", "scripts/server.mjs", "scripts/memo-store.mjs",
                     "scripts/upgrade-server.ps1", "prototypes/web/editor.html"):
        target = isolated / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / relative, target)
    work = isolated / ".work"
    work.mkdir()
    profile = work / "edge-profile"
    (work / "server.json").write_text(json.dumps({"port": 43130, "token": "edge-desktop-test-token"}), encoding="utf-8")
    (isolated / "data").mkdir()
    (isolated / "data/web.txt").write_text("isolated draft", encoding="utf-8")
    counter = folder / "launches.txt"
    fixture = folder / "bootstrap.mjs"
    fixture.write_text(
        "import fs from 'node:fs';\n"
        + f"fs.appendFileSync({json.dumps(str(counter))},'launch\\n');\n"
        + f"await import({json.dumps((isolated / 'scripts/bootstrap.mjs').as_uri())});\n",
        encoding="utf-8")
    powershell = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
    launch = powershell + [str(isolated / "scripts/launch-editor.ps1"), "-BootstrapScript", str(fixture),
                           "-ProfilePath", str(profile), "-Title", "minimal-editor", "-NoLegacyTitle"]
    sequence = 0

    def spawn(command):
        nonlocal sequence
        sequence += 1
        output = folder / f"process-{sequence}-stdout.txt"
        errors = folder / f"process-{sequence}-stderr.txt"
        # Detached browser/helper descendants can inherit pipe handles. Waiting
        # for the launcher PID plus file output does not wait for their EOF.
        with output.open("wb") as out, errors.open("wb") as err:
            process = subprocess.Popen(command, cwd=ROOT, creationflags=subprocess.CREATE_NO_WINDOW,
                                       stdout=out, stderr=err)
        process.test_output, process.test_errors = output, errors
        return process

    def completed(process):
        process.wait(timeout=35)
        output = process.test_output.read_text(encoding="utf-8", errors="replace")
        errors = process.test_errors.read_text(encoding="utf-8", errors="replace")
        assert process.returncode == 0, errors + output
        return json.loads(output)

    # The two first launches race before any window exists. The named mutex
    # must ensure only one reaches bootstrap and the other restores that HWND.
    first = spawn(launch)
    second = spawn(launch)
    results = [completed(first), completed(second)]
    assert sorted(result["reused"] for result in results) == [False, True], results
    handle = results[0]["handle"]
    assert handle and results[1]["handle"] == handle, results
    assert counter.read_text() == "launch\n", "Duplicate browser launch"
    with urllib.request.urlopen("http://127.0.0.1:43130/health", timeout=3) as response:
        assert json.load(response)["app"] == "minimal-editor-v0", "Real server was not started"

    def probe(wanted_profile=profile, title="minimal-editor"):
        command = powershell + [str(isolated / "scripts/focus-editor.ps1"), "-ProfilePath", str(wanted_profile),
                                "-Title", title, "-ProbeOnly", "-NoLegacyTitle"]
        return completed(spawn(command))

    assert probe()["handle"] == handle
    assert not probe(folder / "unrelated-profile")["found"], "Matched an unrelated profile"
    assert not probe(title="Unrelated application")["found"], "Matched an unrelated title"
    gui.show_window(handle, 6)
    gui.eventually(lambda: gui.iconic(handle), "Owned test window did not minimize")
    assert probe()["handle"] == handle
    assert gui.iconic(handle), "Probe unexpectedly restored the window"
    reopened = completed(spawn(launch))
    assert reopened["reused"] and reopened["handle"] == handle, reopened
    gui.eventually(lambda: not gui.iconic(handle), "Launcher did not restore minimized owned window")
    assert counter.read_text() == "launch\n"
    # The explicit browser entry point invokes the same launcher hidden.
    # Exercise that route too without touching the real profile.
    gui.show_window(handle, 6)
    gui.eventually(lambda: gui.iconic(handle), "Owned test window did not minimize again")
    app = spawn(["wscript.exe", str(isolated / "scripts/browser-app.vbs")])
    app.wait(timeout=20)
    assert app.returncode == 0
    gui.eventually(lambda: not gui.iconic(handle), "browser-app.vbs did not restore owned window")
    assert probe()["handle"] == handle
    assert not any(window["class"] == "#32770" for window in gui.windows()), "Unexpected launch error dialog"
    assert (isolated / "data/web.txt").read_text(encoding="utf-8") == "isolated draft"
    gui.checked(gui.post_message(handle, 0x0010, 0, 0))
    return {"passed": True, "desktop": desktop_name.value,
            "concurrent_launches_created_one_window": True, "reopen_restored_same_window": True,
            "unrelated_profile_and_title_ignored": True, "probe_did_not_restore_window": True,
            "actual_bootstrap_started_server": True, "browser_app_vbs_restored_same_window": True}


def parent():
    (ROOT / ".work").mkdir(exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix="edge-desktop-", dir=ROOT / ".work"))
    name = "MinimalEditorEdgeTest_" + uuid.uuid4().hex
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
                                       0x08000004, None, str(ROOT), C.byref(si), C.byref(pi)))
        gui.checked(gui.assign_job(job, pi.process))
        assigned = True
        if gui.resume(pi.thread) == 0xFFFFFFFF:
            raise C.WinError(C.get_last_error())
        assert gui.wait(pi.process, 90000) == 0, "Private desktop launcher test timed out"
        code = W.DWORD()
        gui.checked(gui.get_exit(pi.process, C.byref(code)))
        report = folder / "result.json"
        result = json.loads(report.read_text(encoding="utf-8")) if report.exists() else {"exit_code": code.value}
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
