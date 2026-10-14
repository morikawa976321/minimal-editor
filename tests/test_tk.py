"""Storage regression tests; no visible windows, keyboard or clipboard use."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "tk_editor", Path(__file__).resolve().parents[1] / "prototypes/tk/editor.py")
editor = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(editor)


class DocumentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="minimal-editor-tk-test-")
        self.directory = Path(self.temp.name)
        self.document = editor.Document(self.directory)

    def tearDown(self):
        self.document.close()
        self.temp.cleanup()

    def test_japanese_empty_backup_and_reopen(self):
        text = "日本語の文章。\n" * 2000
        self.document.save(text)
        self.document.save("")
        self.assertEqual(self.document.path.read_bytes(), b"")
        self.assertEqual(self.document.backup.read_bytes().decode("utf-8"), text)
        self.document.close()
        self.document = editor.Document(self.directory)
        self.assertEqual(self.document.text, "")

    def test_exclusive_lock(self):
        with self.assertRaises(editor.AlreadyOpenError):
            editor.Document(self.directory)

    def test_invalid_utf8_is_not_replaced(self):
        self.document.close()
        self.document.path.write_bytes(b"\xff\xfe")
        with self.assertRaises(UnicodeError):
            editor.Document(self.directory)
        self.assertEqual(self.document.path.read_bytes(), b"\xff\xfe")
        # Failed startup released its lock.
        self.document.path.write_bytes(b"recovered")
        self.document = editor.Document(self.directory)
        self.assertEqual(self.document.text, "recovered")

    def test_external_edit_is_not_replaced(self):
        self.document.save("original")
        self.document.path.write_bytes(b"external")
        with self.assertRaises(OSError):
            self.document.save("new")
        self.assertEqual(self.document.path.read_bytes(), b"external")

    def test_failed_replace_preserves_last_good_save(self):
        self.document.save("original")
        original_replace = editor.os.replace

        def fail_main(source, target):
            if target == self.document.path:
                raise PermissionError("test: file locked")
            original_replace(source, target)

        with mock.patch.object(editor.os, "replace", side_effect=fail_main):
            with self.assertRaises(PermissionError):
                self.document.save("new")
        self.assertEqual(self.document.path.read_bytes(), b"original")
        self.assertEqual(self.document.previous, b"original")
        self.assertEqual(list(self.directory.glob("*.tmp")), [])
        self.document.save("new")
        self.assertEqual(self.document.path.read_bytes(), b"new")

    def test_read_failure_is_not_treated_as_empty(self):
        self.document.save("original")
        with mock.patch.object(self.document, "_read", side_effect=PermissionError("test")):
            with self.assertRaises(PermissionError):
                self.document.save("new")
        self.assertEqual(self.document.path.read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()
