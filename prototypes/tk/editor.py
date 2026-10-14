"""A single sheet of text. No dependencies beyond Python's bundled Tk."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import tkinter as tk
from tkinter import messagebox


REPO_ROOT = Path(__file__).resolve().parents[2]
TITLE = "Minimal Editor · Tk"
BACKGROUND = "#181a1b"


class AlreadyOpenError(OSError):
    pass


class Document:
    """Own a document until close; replace complete, flushed UTF-8 files only."""

    def __init__(self, directory: Path):
        directory.mkdir(parents=True, exist_ok=True)
        self.path = directory / "tk.txt"
        self.backup = directory / "tk.txt.bak"
        self.lock = (directory / "tk.lock").open("a+b")
        self.locked = False
        try:
            if os.name == "nt":
                import msvcrt
                self.lock.seek(0, os.SEEK_END)
                if self.lock.tell() == 0:
                    self.lock.write(b"\0")
                    self.lock.flush()
                self.lock.seek(0)
                try:
                    msvcrt.locking(self.lock.fileno(), msvcrt.LK_NBLCK, 1)
                except OSError as error:
                    raise AlreadyOpenError("この文書は既に開いています。") from error
            else:
                import fcntl
                try:
                    fcntl.flock(self.lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                except OSError as error:
                    raise AlreadyOpenError("この文書は既に開いています。") from error
            self.locked = True
            self.previous = self._read()
            self.text = "" if self.previous is None else self.previous.decode("utf-8")
        except BaseException:
            self.close()
            raise

    def _read(self) -> bytes | None:
        try:
            return self.path.read_bytes()
        except FileNotFoundError:
            return None

    @staticmethod
    def _replace(path: Path, payload: bytes) -> None:
        fd, name = tempfile.mkstemp(prefix=path.name + ".", suffix=".tmp", dir=path.parent)
        temporary = Path(name)
        try:
            with os.fdopen(fd, "wb") as output:
                output.write(payload)
                output.flush()
                os.fsync(output.fileno())
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)

    def save(self, text: str) -> None:
        payload = text.encode("utf-8")
        # Never silently replace another application's edits or unreadable data.
        current = self._read()
        if current != self.previous:
            raise OSError("保存先が外部で変更されました。上書きしていません。")
        if payload == self.previous:
            return
        if self.previous is not None:
            self._replace(self.backup, self.previous)
        self._replace(self.path, payload)
        self.previous = payload
        self.text = text

    def close(self) -> None:
        if self.lock.closed:
            return
        if self.locked:
            if os.name == "nt":
                import msvcrt
                self.lock.seek(0)
                msvcrt.locking(self.lock.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl
                fcntl.flock(self.lock.fileno(), fcntl.LOCK_UN)
        self.lock.close()


class Editor:
    def __init__(self, root: tk.Tk, document: Document):
        self.root = root
        self.document = document
        self.last_error: str | None = None
        self.retry_id: str | None = None
        root.title(TITLE)
        root.configure(background=BACKGROUND)
        root.geometry("960x720")
        root.minsize(480, 320)
        self.error = tk.Label(root, background=BACKGROUND, foreground="#ffb4a8",
                              font=("BIZ UDGothic", 11), anchor="w", wraplength=850)
        self.text = tk.Text(
            root, background=BACKGROUND, foreground="#e3e3df",
            insertbackground="#e3e3df", selectbackground="#394249",
            selectforeground="#ffffff", font=("BIZ UDGothic", 15),
            wrap="word", undo=True, autoseparators=True, maxundo=-1,
            borderwidth=0, highlightthickness=0, padx=42, pady=32,
            spacing1=2, spacing3=6, exportselection=False,
        )
        self.text.pack(fill="both", expand=True)
        self.text.insert("1.0", document.text)
        self.text.edit_reset()
        self.text.edit_modified(False)
        self.text.bind("<<Modified>>", self._modified)
        self.text.bind("<Control-s>", self._save_shortcut)
        root.protocol("WM_DELETE_WINDOW", self.close)
        self.text.focus_set()

    def content(self) -> str:
        return self.text.get("1.0", "end-1c")

    def save(self) -> bool:
        try:
            self.document.save(self.content())
        except (OSError, UnicodeError) as error:
            self.last_error = str(error)
            self.root.title(TITLE + " · 未保存")
            self.error.configure(text="未保存 — " + self.last_error)
            self.error.pack(side="bottom", fill="x", padx=42, pady=(0, 12), before=self.text)
            if self.retry_id is None:
                self.retry_id = self.root.after(2000, self._retry)
            return False
        self.last_error = None
        self.root.title(TITLE)
        self.error.pack_forget()
        if self.retry_id is not None:
            self.root.after_cancel(self.retry_id)
            self.retry_id = None
        return True

    def _retry(self) -> None:
        self.retry_id = None
        self.save()

    def _save_shortcut(self, _event: tk.Event) -> str:
        self.save()
        return "break"

    def _modified(self, _event: tk.Event) -> None:
        if self.text.edit_modified():
            self.text.edit_modified(False)
            self.save()

    def close(self) -> None:
        # Failed storage must never silently discard the only in-memory copy.
        if self.save():
            self.document.close()
            self.root.destroy()


def self_test(directory: Path) -> dict:
    """Hidden widget-level smoke test; does not emulate real IME or Handy."""
    if (directory / "tk.txt").exists():
        raise ValueError("--test requires a directory without tk.txt")
    started = time.perf_counter()
    document = Document(directory)
    root = tk.Tk()
    root.withdraw()
    editor = Editor(root, document)
    ready_ms = (time.perf_counter() - started) * 1000
    try:
        payload = "日本語を一人で書く。abc123\n" * 800
        started = time.perf_counter()
        editor.text.delete("1.0", "end")
        editor.text.insert("1.0", payload)
        root.update()
        insertion_save_ms = (time.perf_counter() - started) * 1000
        assert editor.last_error is None, editor.last_error
        assert document.path.read_bytes().decode("utf-8") == payload
        editor.text.insert("end-1c", "続き")
        root.update()
        assert document.backup.read_bytes().decode("utf-8") == payload
        assert document.path.read_bytes().decode("utf-8") == payload + "続き"
        # Exercise Tk's standard Text key-binding insertion procedure without
        # sending keys to the user's desktop or requiring a mapped window.
        editor.text.mark_set("insert", "end-1c")
        root.tk.call("tk::TextInsert", str(editor.text), "日本語入力")
        root.update()
        assert document.path.read_bytes().decode("utf-8") == payload + "続き日本語入力"
    finally:
        root.destroy()
        document.close()
    reopened = Document(directory)
    try:
        assert reopened.text == payload + "続き日本語入力"
    finally:
        reopened.close()
    return {"test": "tk-widget-insert-save-reopen", "characters": len(payload),
            "ready_ms": round(ready_ms, 2), "insert_and_save_ms": round(insertion_save_ms, 2),
            "ime_and_handy": "requires real input verification", "passed": True}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--data-dir", type=Path)
    parser.add_argument("--test", action="store_true")
    args = parser.parse_args()
    if args.test:
        if args.data_dir:
            print(json.dumps(self_test(args.data_dir), ensure_ascii=False))
        else:
            with tempfile.TemporaryDirectory(prefix="minimal-editor-tk-") as temporary:
                print(json.dumps(self_test(Path(temporary)), ensure_ascii=False))
        return 0
    root = tk.Tk()
    root.withdraw()
    document = None
    try:
        document = Document(args.data_dir or REPO_ROOT / "data")
        Editor(root, document)
        root.deiconify()
        root.mainloop()
    except AlreadyOpenError:
        # Repeated launcher clicks must not open stale, competing buffers.
        return 0
    except (OSError, UnicodeError) as error:
        messagebox.showerror("Minimal Editor", "開けません。保存先は上書きしていません。\n" + str(error), parent=root)
        return 1
    finally:
        if document is not None:
            document.close()
        try:
            root.destroy()
        except tk.TclError:
            pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
