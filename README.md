# minimal-editor

AIと独立して一人で文章を書くための、設定機能すら持たないミニマルエディターを自作するプロジェクト(本人の構想 2026-10-14)。

- 常時の規則: [AGENTS.md](AGENTS.md)(親セッションは Codex の astra。権限・委譲先・判断の扱い・本人の基準)
- 依頼: [docs/parent-session-prompt.md](docs/parent-session-prompt.md)
- 状態と再開の入口: docs/STATUS.md(親セッションが作る)

## V0 の入口

この PC では **Win → Minimal Editor → Enter**。比較ページで A・B・C を開く。
`比較を開く.cmd` のダブルクリックでも同じページが開く。
ローカル補助サーバーが動いている間は、生成済み `比較.html` のダブルクリックでも使える。

| V0 | 入力欄 | 本文 |
|---|---|---|
| A · Edge | HTML textarea、Edge app window | `data/web.txt` |
| B · Windows | Rust + Win32 EDIT | `data/native.txt` |
| C · Python | Python + Tk Text | `data/tk.txt` |

本文・バックアップ・ブラウザープロファイルは Git 対象外。設定画面、AI 機能、クラウド連携は無い。

## 再構築

この PC の既存 Node 24、Rust、uv 管理 Python 3.12、Edge を使う。依存パッケージの追加は不要。

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build.ps1
node scripts/bootstrap.mjs --no-open
powershell -ExecutionPolicy Bypass -File scripts/install-shortcut.ps1
```

`install-shortcut.ps1` はスタートメニューに `Minimal Editor.lnk` を1個作る。
再現試験は `powershell -ExecutionPolicy Bypass -File scripts/test.ps1`。GUI は非表示、テスト本文は `.work` に隔離。
測定条件と限界は [検証記録](docs/verification.md)、判断は [判断ログ](docs/decisions.md) を参照。
