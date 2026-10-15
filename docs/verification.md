# 検証記録 · 2026-10-15

実機 Windows、Node v24.11.1、rustc 1.98.1、uv 管理 Python 3.12 + Tk 8.6、既存 Edge。外部パッケージ追加なし。空きRAMは初期確認6.28GB、続く確認5.96GB。GUI検証とビルドは順番に実行し、ブラウザーは `--headless=new --disable-gpu`。画面前面化・グローバルなキー送出・クリップボード操作なし。

## 結果

| 項目 | A · Edge | B · Windows | C · Python |
|---|---|---|---|
| 日本語の入力経路 | CDP `Input.insertText` | EDITへのUnicode `WM_CHAR` と `EM_REPLACESEL` | `Text.insert` と標準キーbindingの `tk::TextInsert` |
| 変換イベント | `Input.imeSetComposition` で未確定表示→確定文字列の保存を確認 | OS 標準EDITのIME処理を使用 | Tk TextのIME処理を使用 |
| 長文 | 15,300字 | 12,521字（最終本文） | 13,600字。強制終了試験は15,000字 |
| 保存後の強制終了→再開 | Node/専用Edgeを強制終了、新ブラウザーで一致 | 保存済みプロセスを強制終了、再読込一致 | 保存済み子プロセスをkill、再読込一致 |
| 保存保護 | UTF8分割送信、版競合、外部変更拒否、バックアップ | 不完全サロゲート、保存エラー・閉じる拒否、復帰、外部変更拒否 | 破損UTF8・読込失敗・外部変更・置換失敗・閉じる拒否 |
| 同時編集 | 2つ目の編集権取得を拒否 | 2プロセス目を拒否、kill後は再取得可 | 排他ロックを検証 |
| 起動 | 各実行対象の存在とパス確認。比較HTMLの起動要求を検証 | hidden実行・probeを実行 | hidden Tkを実行 |

**実IMEの候補窓・インライン表示と、本物のHandyによるOS入力は未検証。** CDP / WM_CHAR / Tk binding は対象ウィジェットの入力処理を通すが、フォアグラウンドでの SendInput、Handy のフォーカス復帰、実IMEの候補窓の位置を証明しない。本人の操作を奪わない制約を守り、本人の試用へ残した。

構造の根拠: [Microsoft の IME 概説](https://learn.microsoft.com/en-us/windows/apps/develop/input/input-method-editors) は edit control への IME 入力を説明し、[標準 EDIT の仕様](https://github.com/MicrosoftDocs/win32/blob/docs/desktop-src/Controls/edit-controls.md) には IME 状態メッセージがある。[Tk の caret 仕様](https://www.tcl-lang.org/man/tcl8.5/TkCmd/tk.htm) は Windows IME の位置決めへの利用を明記する。これらは採用構造の根拠であり、この PC の実IMEが正しく動いたという実測証拠ではない。

## 測定値（閾値ではない）

単発のスモーク測定。OSキャッシュ、測定開始点、入力方式が違うため、方式間の公平な起動ベンチマークではない。

| 指標 | 実測値 | 測定範囲 |
|---|---:|---|
| A 入力準備 | 539.97ms | 専用headless Edge起動開始→入力可能。Node起動時間は除く |
| A 長文の投入と保存 | 546.62ms | 15,300字のCDP一括投入から本文ファイル一致まで。50ms間隔の照合を含む |
| A 長文後の1文字入力 | 中央値7.87ms / p95 13.04ms | 20回のCDP Input.insertText完了時間。各回の保存確認を挟む。OSキー入力の遅延ではない |
| B hidden入力準備 | 5.566ms | Rust main開始→hidden EDIT作成・読込 |
| B 長文の投入と同期保存 | 14.144ms | 12,500字のEM_REPLACESEL→fsync/置換完了 |
| B 追記と同期保存 | 10.833ms | 長文への文字列追記 |
| B Unicode文字列の入力と同期保存 | 67.939ms | 改行・日本語・ASCII・サロゲートの一連のWM_CHAR。後続の障害注入2文字を除く |
| C hidden入力準備 | 220.90ms | Python自己試験内→rootとText作成。Pythonプロセス起動時間は除く |
| C 長文の投入と保存 | 22.40ms | 13,600字のText.insert→イベント処理と同期保存 |

「1万字で引っかからない」の合格閾値は本人から与えられていない。今回の数値を提示し、体感まで検証済みとはしない。Aの長文一括投入は約0.55秒で、長文後の単文字入力とは分けて扱う。

## 実行したテスト

- `node --test tests/server.test.mjs`：HTTP保存/空保存/バックアップ、HTTPチャンクを日本語1文字の途中で分割、同時編集、版競合、外部変更、Origin/Host/認証、不正起動名、破損UTF8。
- Python unittest 9件：`test_tk.py` 6件と `test_tk_ui.py` 3件、全PASS。Tk標準入力binding検証を追加後にも全PASS。
- `tests/native.ps1`：native組込試験、2プロセス排他、保存直後の強制終了と再読込、破損UTF8を変更せず拒否、全PASS。
- `node tests/web-ui.mjs`：CDP文字入力・変換・保存、保存後のサーバー/ブラウザー強制終了と再開、サーバー停止中の追記→生きているページの再読込から復元、比較選択結果、file://比較ページからの起動要求、全PASS。
- A と比較画面のスクリーンショットを取得して目視確認。暗色・余白・結果欄・コピーボタンの収まりを確認した。B/C はhiddenウィジェットを検証し、見た目の実画面確認は本人待ち。
- ショートカットの TargetPath / Arguments / WorkingDirectory を読み戻し、依頼どおり1ファイルを作成したことを確認。本人の Win キー入力やスタート検索結果の実画面は操作していない。

最終ブラウザー測定と画像: `.work/web-ui-tA2fFc/result.json`、同ディレクトリの `editor.png` / `comparison.png`。ローカルの検証生成物であり Git 対象外。

## 発見して修正した問題

1. HTTP Bufferをチャンクごとに文字列化すると日本語が壊れる。バイト列を連結し、厳格UTF8 decodeするよう修正。文字途中分割の回帰試験PASS。
2. 一時的なheartbeat失敗で永久に入力不能となる。通信失敗と編集権失効を区別し、前者は本文を保持、後者はコピー可能なreadOnlyに変更。
3. 複数画面が共通の未保存キーを消し合う。画面ごとのキーに分離し、復元する別稿を先にファイルへ退避。
4. Win32の再入可能な呼び出し中にRustのState参照を保持する設計を見直し、callbackをまたぐ箇所をraw pointerで扱うよう修正。
5. 保存成功後に応答だけ失うと版がずれて再送が永久失敗する。未確認のリクエストを保持して同一内容で再送し、サーバーが保存済み内容への再送を受理するよう修正。CDPで保存成功後のfetch応答を故意に失敗させ、さらに追記して保存復帰する試験PASS。
6. Aの破損UTF8で比較サーバー全体が起動不能になる。読込エラーをAのAPI内に隔離し、正常性確認を原稿読込から分離。破損本文を保持したままBの起動要求が通り、修復後にAが読み直せる試験PASS。
7. Bに標準EDITだけでは不足するCtrl+Aを追加。全選択を検証し、通常A・AltGr+A・IMEメッセージは消費しない。Cは実環境のTk標準bindingにCtrl+Aがあることを確認した。

## 観測した限界

- Aでサーバー停止中に6文字を追記し、直後にブラウザーも強制終了した試験では、localStorageの未保存コピーが復元されなかった。保存済み本文は残った。localStorageへの書込API完了をディスク耐久性の保証と見なさない。保存失敗中の同時クラッシュまで「直前の全文が残る」とは言わない。
- B/Cも入力イベント処理やfsync完了前のkill、IME未確定文字、電源断までは保証しない。試験は保存完了後のプロセス強制終了。
- 3方式の `.bak` は直前の1版だけ。保存履歴の一覧ではない。
- file://の比較HTMLから各アプリを開くには補助サーバーが必要。PC再起動後は1個のショートカットから起動する。

## 本人試用後の起動不具合 · 2026-10-15

本人報告でBの初回起動が非表示になっていたことが判明。旧実装の `windowsHide:true` がSTARTUPINFOのSW_HIDEとなり、標準ShowWindowの初回に優先されていた。再クリックで起動した2個目だけがロックエラーを表示した。最初のhidden試験ではこの経路を検出できていなかった。

修正後は `tests/gui-launch.mjs` が実ランチャーと実exeの `--probe` を使い、旧指定のSTARTUPINFO `{flags:257,show:0}`、新指定 `{flags:257,show:10}` を確認する。probe自身は表示しない。nativeの保存・強制終了・排他・Ctrl+A回帰試験もPASS。通常Bは古い起動元がSW_HIDEを付けても表示を再試行する。

加えて `tests/gui_desktop.py` を親が実行し、本人の画面とは異なるランダム名のWindows desktopに通常GUIを起動して確認した。子が実際に接続されたdesktop名を照合し、SwitchDesktopは使わない。新ランチャー・旧SW_HIDE指定の両方でウィンドウ表示、最小化後の再起動で同じHWNDへ復帰、重複ウィンドウ0、エラーダイアログ0。結果は `.work/gui-desktop-s8o9tp4a/result.json`。試験専用ジョブの子プロセスだけを片付ける。sandboxではdesktop作成が拒否されるため、この試験は権限付きで実行した。

## A採用・minimal-editorの実用入口 · 2026-10-16

通常起動を本文直結へ変更し、タイトルを `minimal-editor` に固定。HTTP保存試験とEdge headless試験を再実行しPASS。15,300字、CDPの日本語変換、保存済み原稿のサーバー/ブラウザー強制終了後の復帰、未保存ジャーナルからの再読込、保存応答喪失後の追記を確認。今回の結果は `.work/web-ui-QUUp2q/result.json`。試験前の空きRAMは10.72 GiB、GUI試験は直列実行。

起動対象の識別は専用プロファイル・Edgeのウィンドウクラス・タイトルをすべて照合する。実環境では `focus-editor.ps1 -ProbeOnly` の読み取り専用確認を行い、画面の前面化・キー・クリップボード操作はしていない。

`tests/edge_desktop.py` を親が最終版で実行しPASS。実bootstrap/server/editor/VBS/PowerShellを `.work` の独立コピーに配置し、port43130・テスト原稿・独立プロファイル・切替しない専用desktopで検証した。同時初回2起動でbootstrap1回と同一HWND、最小化から同じウィンドウへ復帰、誤profile/titleの検出拒否、ProbeOnlyが最小化解除しないこと、実app.vbs経由の復帰、テスト原稿不変を確認。結果は `.work/edge-desktop-p6uccpze/result.json`。サーバーとEdgeを含む試験所有のプロセスはjob終了で片付けた。

初回の試験はPowerShellの `communicate()` でタイムアウトした。出力をファイルへ取り、ランチャーPIDの終了を直接待つ方式に変えると成功。継承されたPIPEのEOF待ちが有力だが、初回のプロセス終了状態を記録していないため原因を断定しない。診断時のウィンドウ復帰APIは0〜5msで戻っていた。本番の起動コードはこの切り分けで変更していない。

スタートメニューの `minimal-editor.lnk` は作成後にTargetPath / Arguments / WorkingDirectoryを読み戻し、旧 `Minimal Editor.lnk` が残っていないことを確認。Winキー検索の実画面を自動操作したという意味ではない。

## 外観調整 · 2026-10-16

CSSだけの変更につき保存・起動の全試験は繰り返さず、実CSSを抽出したテスト用サンプルを専用headless Edgeで描画。幅1400px時にtextarea1100px、幅500px時にtextarea/ページとも500px、背景rgb(32,32,32)、本文rgb(223,223,223)を確認。CDPの実描画フォントは英字Intel One Mono、日本語BIZ UDGothic。スクリーンショット `.work/style-check-omP9pD/wide.png` を目視確認。テストページ・プロファイルは `.work` のみ、本人の原稿は読んでいない。

## 通常保存を無表示に · 2026-10-16

`tests/web-ui.mjs` に保存要求を意図的に保留する検査を追加。状態欄が空のまま、キャンセル可能なbeforeunloadイベントが未保存時にはpreventDefaultされ、保存完了後にはされないことを確認した。実際のブラウザーの確認ダイアログをクリックした試験ではない。従来の日本語入力、保存失敗表示、応答喪失後の再試行、強制終了後の保存済み本文復帰もPASS。結果は `.work/web-ui-9rLMsj/result.json`。この回の初回入力時間には保留中の確認処理が含まれるため、以前の速度測定とは直接比較しない。

## 原稿切替プレビュー · 2026-10-16

visualizeのラッパー内で3案を実際のカルーセル操作で選び、開閉・原稿切替・メモリー上の編集保持を確認。「中央の検索」の絞込みと新規作成も確認。360pxの外側画面に対して内側313pxで横溢れなし。3案の画像を目視確認。結果と画像は `.work/preview-check-0zrjCR/`。本番の原稿・保存APIに接続するプレビューではなく、複数原稿の永続保存を検証したものではない。
