# STATUS · 2026-10-15

## 現在

3方式の動くV0と比較ページを作成。本人の試用待ち。本人の「寝かせる」を変更していない。本人の入力・好み以外の今回の実装と非表示検証を完了。

| 表示 | 実装 | 起動対象 | 本文 |
|---|---|---|---|
| A · Edge | HTML textarea + loopback Node | Edge app window | `data/web.txt` |
| B · Windows | Rust + Win32 EDIT | `build/native.exe` | `data/native.txt` |
| C · Python | Tk Text | `prototypes/tk/editor.py` | `data/tk.txt` |

各原稿は独立。設定なし、UTF-8テキスト、自動保存、直前バックアップ。AI機能なし。本人用の原稿は空で用意し、テスト原稿は `.work` に隔離した。

## 現物の開き方

**Win → `Minimal Editor` → Enter**。比較ページの「開く」から各V0を起動する。

同じ入口は `C:\Users\tm976\mf\minimal-editor\比較を開く.cmd` のダブルクリック。
生成済み `比較.html` は補助サーバー稼働中ならダブルクリックで使える。PC再起動後は上の入口で補助サーバーも起動する。

作成したリポジトリ外のファイルは、依頼で個別許可された次の1個:

`C:\Users\tm976\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Minimal Editor.lnk`

Target は `C:\Windows\System32\wscript.exe`、引数はリポジトリ内 `scripts\launch.vbs`。レジストリ・サービス・自動起動登録・グローバルパッケージ導入はしていない。

## 検証と残る限界

Tk 9試験、HTTP試験、native hidden/強制終了試験、Edge headless試験を通過。保存済みの日本語1万字超は強制終了→再開で一致。AはCDP変換イベント、BはUnicode WM_CHAR、CはTk標準入力bindingを検証。詳細と測定値は [verification.md](verification.md)。

実IMEの候補位置・インライン表示、本物のHandy、見た目/手触り、Winキー検索の実操作は本人待ち。自動テストの成功をこれらの実測に置き換えない。

Aは保存失敗中にブラウザーまで即killすると未保存追記が失われる場合がある。保存中・失敗時は画面に表示し、閉じる際は未保存確認が出る。`.bak` は直前1版だけ。OS停止・電源断は検証していない。

## 次の一回

比較ページの判断カード: **「使い続けたいのはどれ？」**。Aを暫定推奨するが、B/Cは捨てていない。各案で同じ文章を入力し、IMEとHandyも試し、「一番いい」または「どちらも問題ない」などをクリックする。「結果をコピー」で出る1ブロックを次のメッセージに貼る。保留可。サイズ・行幅・保存先を変えたければ一言。

## 再開する親へ

1. AGENTS.md、判断ログ末尾の本人待ち、本人の返答を読む。返答は原文のまま `docs/decisions.md` へ追記。
2. 先にGit状態と実ファイルを確認。`data` / `.work` / Edge専用プロファイルには本人の原稿があり得る。リセット・削除しない。
3. 入力の致命的問題があれば方式決定より先に修正。問題のある構造を体感評価で押し切らない。
4. 採用後はその版の入口へショートカットを直結し、他案の原稿は保持。複数原稿や公開連携を無断追加しない。
5. `scripts/build.ps1` でnative再構築、`scripts/test.ps1` で再現試験。重い処理前には空きRAMを確認する。

補助サーバーは `127.0.0.1:43127`、Nodeプロセス1個。設定・ログは `.work/server.json` / `.work/server.log`。終了が必要なら `/health` のPIDとコマンドラインがこのリポジトリの `scripts/server.mjs` であることを確かめ、そのPIDだけを終了する。アプリは次の入口起動で復帰する。
