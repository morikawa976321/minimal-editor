# minimal-editor

AIと独立して一人で文章を書くための、設定機能すら持たないミニマルエディターを自作するプロジェクト(本人の構想 2026-10-14)。

- 常時の規則: [AGENTS.md](AGENTS.md)(親セッションは Codex の astra。権限・委譲先・判断の扱い・本人の基準)
- 依頼: [docs/parent-session-prompt.md](docs/parent-session-prompt.md)
- 状態と再開の入口: docs/STATUS.md(親セッションが作る)

## 使い方

この PC では **Win → minimal-editor → Enter**。本文が直接開き、自動で保存される。
A（Edge app window）を本人が採用。タブやアドレスバーのない通常ウィンドウで、全画面固定のキオスクではない。
すでに開いている場合は、そのウィンドウに戻る。設定画面はない。

本文は `data/web.txt`（UTF-8）、直前の保存版は `data/web.txt.bak`。Aで試した原稿をそのまま継続する。
比較用のB/Cとその原稿は保持している。`比較を開く.cmd` は引き続き比較ページを開く。

| V0 | 入力欄 | 本文 |
|---|---|---|
| A · Edge | HTML textarea、Edge app window | `data/web.txt` |
| B · Windows | Rust + Win32 EDIT | `data/native.txt` |
| C · Python | Python + Tk Text | `data/tk.txt` |

本文・バックアップ・ブラウザープロファイルは Git 対象外。設定画面、AI 機能、クラウド連携は無い。

## 再構築

普段の起動は、この PC の既存 Node 24、Edge、Windows PowerShellを使う。依存パッケージの追加は不要。
Rust / Python は旧B/Cとその試験だけに必要。

```powershell
node scripts/bootstrap.mjs --no-open
powershell -ExecutionPolicy Bypass -File scripts/install-shortcut.ps1
```

`install-shortcut.ps1` はスタートメニューに `minimal-editor.lnk` を1個作る。旧比較用 `Minimal Editor.lnk` がこのリポジトリのものと確認できた場合だけ置き換える。
比較版を再構築する場合は `powershell -ExecutionPolicy Bypass -File scripts/build.ps1`。比較ページは `node scripts/bootstrap.mjs --compare`。
再現試験は `powershell -ExecutionPolicy Bypass -File scripts/test.ps1`。GUI は非表示、テスト本文は `.work` に隔離。
測定条件と限界は [検証記録](docs/verification.md)、判断は [判断ログ](docs/decisions.md) を参照。
