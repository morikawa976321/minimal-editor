# minimal-editor 開発ガイド

> 外から読む人向けの案内はリポジトリ直下の [README.md](../README.md)。この文書は開発を続けるための詳細。

2026-10-18: 本人の選択でRust/Win32/Rich Editのネイティブ版を採用。保存処理は別スレッドで行い、通常の自動保存中も入力できる。ブラウザ版は **`ブラウザ版.vbs`** で開ける。詳細は [両版の案内](native-comparison.md)、[メモリの検討](native-memory-options.md)。

AIと独立して一人で文章を書くための、設定機能すら持たないミニマルエディターを自作するプロジェクト(本人の構想 2026-10-14)。

- 常時の規則: [AGENTS.md](../AGENTS.md)(親セッションは Codex の astra。権限・委譲先・判断の扱い・本人の基準)
- 依頼: [docs/parent-session-prompt.md](parent-session-prompt.md)
- 状態と再開の入口: docs/STATUS.md(親セッションが作る)

## 使い方

この PC では **Win → minimal-editor → Enter**。本文が直接開き、自動で保存される。
ネイティブ版の通常ウィンドウが開く。ブラウザやNodeの常駐は不要。
すでに開いている場合は、そのウィンドウに戻る。設定画面はない。

本文は `data/native-next/memos/<ID>.txt`（UTF-8）、一覧は `data/native-next/catalog.tsv`。バックアップ・履歴・保存失敗時の復元用コピーを保持する。終了時は最後の保存が成功してから閉じる。
採用時にブラウザの15件を照合し、14件を維持、差がある1件を追加で取り込んだ。元の比較版の本文は「（移行前の比較版）」を付けてアーカイブに保持。移行前全体の退避は `data/native-adoption/2026-10-18/native-before/`。

ブラウザ版と旧B/Cも保持している。ブラウザの本文は従来の `data/web.txt` と `data/memos/` に残り、ネイティブ版とは自動同期しない。`ネイティブ比較.vbs` で両版を開いて比べられる。旧比較ページは `比較を開く.cmd`。

| V0 | 入力欄 | 本文 |
|---|---|---|
| 採用版 · Native | Rust + Win32 Rich Edit | `data/native-next/memos/<ID>.txt` |
| A · Edge（保持） | HTML textarea、Edge app window | `data/web.txt`、`data/memos/<ID>.txt` |
| B · Windows | Rust + Win32 EDIT | `data/native.txt` |
| C · Python | Python + Tk Text | `data/tk.txt` |

本文・バックアップ・ブラウザープロファイルは Git 対象外。設定画面、AI 機能、クラウド連携は無い。

## 再構築

普段の起動はWindowsのVBSから構築済みexeを直接実行する。Rustは再構築用で、実行には不要。ブラウザ版には既存Node 24・Edge・PowerShellを使う。追加の依存パッケージはない。PythonはGUI検証と旧Cに使う。

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-native.ps1
powershell -ExecutionPolicy Bypass -File scripts/install-shortcut.ps1
```

`install-shortcut.ps1` はスタートメニューに `minimal-editor.lnk` を1個作る。旧比較用 `Minimal Editor.lnk` がこのリポジトリのものと確認できた場合だけ置き換える。
比較版を再構築する場合は `powershell -ExecutionPolicy Bypass -File scripts/build.ps1`。比較ページは `node scripts/bootstrap.mjs --compare`。
ネイティブの再現試験は `powershell -ExecutionPolicy Bypass -File scripts/test-native.ps1`。ブラウザ版と旧試作の再現試験は `scripts/test.ps1`。GUI は非表示、テスト本文は `.work` に隔離。
測定条件と限界は [検証記録](verification.md)、判断は [判断ログ](decisions.md) を参照。
