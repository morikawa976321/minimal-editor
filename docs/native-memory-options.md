# ネイティブGUIと10MBの検討 · 2026-10-18

## 結論

**TUIだけに限られない。** 今回作ったRust/Win32/Rich Editのネイティブ比較版は、14,700字で専用RAM 4.090–4.168MiB、40回の追記後も4.293–4.520MiBだった。この測定状態では専用RAMが10,000,000 bytes未満。ただし共有ページ込みは約29〜30MiBであり、すべての物理RAMを10MB以内にした実装ではない。実IME入力中や長時間利用の上限保証とも区別する。旧試作の測定は [memory.md](memory.md)。

現行の使い心地をできるだけ保つ第一候補は、**Rust単一プロセス＋Win32＋Windows標準Rich Edit**。ブラウザ・Node・新しいGUIランタイムを常駐させず、本文の入力機能はOSに任せる。これは構成上の判断であり、10MBを保証する公式仕様や一般的なメモリ使用量の引用ではない。

## 今回の実測

Windowsの表示しない専用desktopで、同じ本文14,700字、各条件3起動、準備完了後0.25秒待機し0.15秒間隔3点を測定。各表セルは9点の最小〜最大。新しいGUIランタイムや依存パッケージは導入していない。空きRAMは開始時2.492GiBで、ブラウザは併走せず、1個ずつ起動した。

| 条件 | 専用RAM MiB | 共有込みRAM MiB | 専用コミット MiB |
| --- | ---: | ---: | ---: |
| ネイティブ比較版、1メモを開いた待機状態 | 4.090–4.168 | 29.129–29.250 | 5.355–5.453 |
| 同版、40回追記・保存後（15,100字） | 4.293–4.520 | 29.484–29.781 | 5.562–5.895 |
| 最小のGDI描画窓（編集不可） | 1.062–1.094 | 7.922–8.477 | 1.535–1.539 |
| 標準EDIT部品だけのprobe | 3.500–3.543 | 25.051–25.141 | 4.742–4.789 |
| Rich Edit部品だけのprobe | 3.398–4.070 | 25.855–28.562 | 4.594–5.379 |
| 描画・入力なしの待機プロセス | 0.379 | 3.902 | 0.617 |

ネイティブ比較版は**テストモードとレポート出力を無効**にし、比較ページ・Node補助・Python測定器を製品値に含めていない。製品が必要とする常駐プロセスはこの1個。初回のブラウザ原稿コピー時は短時間だけNodeを使い、ネイティブ単独VBSでは2回目以降exeへ直接渡す。起動時の一時的なコピー処理を含むピークは未測定。

本文20px・行間36px・英数字/日本語のフォント関連付け、中央一覧、改名、アーカイブ・取消・復元、自動保存・履歴を持つ実装で測った。ただし測定データは1メモで、実IME/Handy、数時間のUndo履歴、多数・巨大メモ、他PCの8GB環境を試した値ではない。起動ピークや測定間のピークを上限として保証しない。作為的なworking setの追出しは使っていない。

40回×3起動の10文字追記は実Rich Editへのメッセージと同期保存を通し、ファイルの最終本文も検証。入力要求から保存完了まで120回の中央値13.383ms、p95（nearest rank）17.991ms、最大45.129ms。これはこのPCの測定結果であって、本人のタイピング体感や遅いディスクでの保証ではない。数値を合否の閾値として使っていない。

**共有込み10MBについて**: 編集しないGDI窓は8,888,320 bytes以下だったので、「Windowsでウィンドウを出すだけで10MBは不可能」とは言えない。ただし、この窓にはIME・選択・Undo・保存がない。GUIのまま文字入力を自作する余地はあるが、その機能を足しても10MB未満に収まる実証はない。今回の実用候補は標準入力部品の品質を優先した。

再現は `tests/native_memory_desktop.py`、生データは `.work/native-memory-27vahevi/result.json`（Git対象外）。Rust 1.98.1、追加crateなし、opt-level=s／codegen-units=1／debuginfo=0。測定exeは323,584 bytes、SHA-256 `47fd7271e059b993cf06bb95e0b73ad559d01f03924d43ea629eec554250b711`。比較probeは181,760 bytes、SHA-256 `48d1016b2ec76d3f28373f4a6d0ab44ad864205bae7d359ac1846a943189aebd`。exeサイズをRAMの代用にはしない。

## 何を10MBと呼ぶか

| 指標 | 意味 | 判定に使うときの注意 |
| --- | --- | --- |
| private working set（専用RAM） | そのプロセスだけが使う、現在RAMにあるページ | 専用の常駐負荷の比較に使う。共有DLL等は含まない |
| working set（共有込みRAM） | そのプロセスの現在RAMにあるページ | OS部品等の共有ページも含む。複数プロセスを足すと共有ページを二重に数えることがある |
| private commit（専用コミット） | プロセス向けに確保された専用のコミット量 | 現在RAMにある量とは異なる。小さい専用RAMだけで保存・Undo等の確保量まで小さいと判断しない |

定義はMicrosoftの [PROCESS_MEMORY_COUNTERS_EX2](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex2) と [Working Set](https://learn.microsoft.com/en-us/windows/win32/memory/working-set) に基づく。MBを厳密に扱うなら10MB＝10,000,000 bytes、表の単位MiBは1,048,576 bytes。10MiBへの読み替えで目標を緩めない。

測定では3指標と対象プロセスを併記する。最小化・`EmptyWorkingSet`等による強制追出しで小さく見せない。OSのメモリ圧迫でもworking setは変動するため、同一PC・本文・ウィンドウ寸法・操作状態を揃える。共有込みRAMはアプリを閉じたときにその全量が空くという値ではなく、専用RAMはアプリが利用する全物理メモリという値でもない。

## 構成の比較

| 構成 | 現行UIとの相性 | 10MBについて確認できること・限界 |
| --- | --- | --- |
| Rust＋Win32＋Msftedit Rich Edit | 暗色、余白、中央一覧、ショートカット、文字書式を組める。IME・選択・Undoは既存部品を使える | 今回の比較版で専用RAM約4〜5MiB。実IMEと長時間のUndo履歴、多数メモの状態はさらに本人の利用で確かめる |
| Rust＋Win32標準EDIT | ブラウザなしで本文を編集できる | 旧試作の専用RAMは10MB未満を実測。以前のIME未確定文字反転と、文字組み・操作の差が残り、メモリだけを理由に採用しない |
| Rust等＋GDIによる自前入力部品 | 文字配置・描画の裁量は大きい | 描画だけなら共有込み10MB未満のprobeあり。TSF、IME、選択、Undo、折返し、アクセシビリティの実装が必要で、使える入力部品の値は未測定 |
| RustのTUI＋既存terminal | キー操作・暗色は組める。現行のピクセル単位の余白・文字組み・一覧・スクロールバーは同じ形で再現しにくい | エディター本体だけ小さくても、表示・入力を担うterminal／console hostを含める必要がある。10MB以内の証明にはならない |
| WebView2等の埋込みブラウザ | HTML/CSSを再利用しやすい | ホストexeの値だけでは不十分。browser・renderer等の関連プロセスを数える。今回の軽量化の第一候補にしない |

Rich EditにはUnicode、インラインIME、多段Undo、文字・段落書式があり、WindowsのMsftedit.dllはRich Edit 4.1とTSFを提供する。[MicrosoftのRich Edit解説](https://learn.microsoft.com/en-us/windows/win32/controls/about-rich-edit-controls)。これらのAPIがあることは、ブラウザの行間・字間・フォントフォールバック・IME配色を同じにできたという実証ではない。

自前入力部品には実装の逃げ道があるが、例えばTSFの [ITextStoreACP](https://learn.microsoft.com/en-us/windows/win32/api/textstor/nn-textstor-itextstoreacp) は、本文の読書きだけでなく選択、ロック、画面上の文字位置等をアプリ側に求める。GDIで文字を描くだけでは日本語の入力部品にならない。実装量と品質の負担が増えるというのがこのプロジェクトでの評価であり、公式文書がメモリ量を約束しているわけではない。

WebView2はホストのほかにbrowser・renderer・GPU等のプロセスを持つ。[Microsoftのプロセスモデル](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)。GPUを使わない構成を含め、関連プロセスを省いて比較しない。

## TUIなら必ず小さくなるか

Windowsのterminalは、文字を受け取り、画面を描き、入力を受けるGUIでもある。Microsoftの構成説明では、アプリ本体とconsole server／terminalは別の役割で、conhost.exe／OpenConsole.exe／Windows Terminalがそれを担う。[ConsoleとTerminalの構成](https://learn.microsoft.com/en-us/windows/console/ecosystem-roadmap)。

したがってTUIを測る場合は、本体に加え、その起動のために必要になったterminal・console host・残るshellも記録する。すでに開いているterminalを共用する場合は、単独起動時の総量と、既存terminalに追加したときの増分を分ける。後者だけを「TUI全体」と呼ばない。既存terminalが別プロセスに接続を集約することもあるため、単純な親子PIDだけで見落とさない。

今回の`console-worker`測定モードは、本文を保持して待機するだけで、描画・入力・保存・Undoがなく、terminalにも接続しない。GUI APIを遅延ロードすることで、**GUIを持たないプロセスの下限を探る対照**にはなるが、使えるTUIのメモリ使用量ではない。

**astra の判断**: TUIは希望する見た目と操作に制約を加える一方、関連プロセス込み10MBという利益は未確認。軽量化のために先にTUIへ切り替える根拠はない。まず標準Rich EditのGUI版を比較し、専用RAMの改善と使用感が両立するかを確かめる。共有込みで厳密10MBが最優先になった場合だけ、さらに低いGUI構成と実TUI全体を同条件で比較する。

## 最小構成の比較用probe

`tests/memory_probe.rs` は依存を追加しない測定用ソース。製品ではない。親の専用desktop試験からだけ起動し、本人の画面に表示しない。

```powershell
rustc --edition=2021 -O tests/memory_probe.rs -o build/memory-probe.exe
```

共通引数は `--mode <mode> --hold-ms 15000 --data-dir <隔離試験フォルダー>`。フォルダーの`probe.txt`をUTF-8で読み、初期描画後に`ready.json`へPIDとウィンドウハンドルを出す。規定時間で終了する。上記はコンパイル手順で、表示desktopで直接実行する手順ではない。

| mode | 内容 |
| --- | --- |
| bare | 通常のWin32ウィンドウにGDIで本文を描画。編集はできない |
| edit | 標準EDIT。複数行・折返しあり |
| richedit | OSのMsftedit.dll／RICHEDIT50W。プレーンテキスト・複数行・折返しあり |
| console-worker | GUIをロードせず同じ本文を保持して待機。実TUIではない |

GUI3方式は960×740、BIZ UDGothic 20px、同じ本文・描画領域。Rich EditとEDITで本文の意味は同一だが、内部改行表現・フォントフォールバック・折返し位置まで同一とは保証しない。GUI部品の追加費用を調べるprobeであり、フォント2種・文字組み・自動保存・メモ一覧等を持つ完成試作の値を代用しない。

このprobeだけで「WindowsのGUI全体に10MBを切れない下限がある」と一般化しない。x86化、Rust標準ライブラリを外す実装、読み込むDLLの選別等は別の実験候補だが、それらがこのPCで目標を満たす実測はなく、exeファイルの小ささからRAMの小ささを推定しない。
