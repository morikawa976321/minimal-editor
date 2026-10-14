"""Hidden Tk tests. No system keystrokes, visible windows, or clipboard access."""

import importlib.util
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock


MODULE_PATH = Path(__file__).resolve().parents[1] / "prototypes/tk/editor.py"
SPEC = importlib.util.spec_from_file_location("tk_editor", MODULE_PATH)
editor = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(editor)


class HiddenWidgetTests(unittest.TestCase):
    def test_insert_save_and_reopen(self):
        with tempfile.TemporaryDirectory(prefix="minimal-editor-tk-ui-") as temporary:
            result = editor.self_test(Path(temporary))
            self.assertGreater(result["characters"], 10000)
            self.assertTrue(result["passed"])

    def test_failed_close_keeps_text_and_window_alive(self):
        with tempfile.TemporaryDirectory(prefix="minimal-editor-tk-ui-") as temporary:
            document = editor.Document(Path(temporary))
            root = editor.tk.Tk()
            root.withdraw()
            window = editor.Editor(root, document)
            try:
                window.text.insert("1.0", "まだ保存できていない文章")
                with mock.patch.object(document, "save", side_effect=PermissionError("test")):
                    root.update()
                    window.close()
                self.assertTrue(root.winfo_exists())
                self.assertEqual(window.content(), "まだ保存できていない文章")
                self.assertIsNotNone(window.last_error)
                self.assertTrue(window.save())
                self.assertEqual(document.path.read_text(encoding="utf-8"), window.content())
            finally:
                root.destroy()
                document.close()

    def test_saved_widget_text_survives_forced_process_termination(self):
        # Wait only for the completed modification callback, then terminate
        # without Editor.close / Python cleanup / Tk's window-close callback.
        with tempfile.TemporaryDirectory(prefix="minimal-editor-tk-kill-") as temporary:
            script = "\n".join([
                "import runpy, pathlib",
                f"m = runpy.run_path({str(MODULE_PATH)!r})",
                f"d = m['Document'](pathlib.Path({temporary!r}))",
                "r = m['tk'].Tk(); r.withdraw()",
                "e = m['Editor'](r, d)",
                "e.text.insert('1.0', '\\u65e5\\u672c\\u8a9e' * 5000)",
                "r.update()",
                "assert e.last_error is None, e.last_error",
                "print('saved', flush=True)",
                "r.mainloop()",
            ])
            process = subprocess.Popen([sys.executable, "-c", script],
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       text=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            lines = queue.Queue()
            reader = threading.Thread(target=lambda: lines.put(process.stdout.readline()), daemon=True)
            reader.start()
            try:
                self.assertEqual(lines.get(timeout=20).strip(), "saved")
            finally:
                process.kill()
                _, errors = process.communicate(timeout=10)
                reader.join(timeout=2)
            self.assertEqual(errors, "")
            reopened = editor.Document(Path(temporary))
            try:
                self.assertEqual(reopened.text, "日本語" * 5000)
            finally:
                reopened.close()


if __name__ == "__main__":
    unittest.main()
