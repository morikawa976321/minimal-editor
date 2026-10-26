# minimal-editor icon

2026-10-26、本人の「1のカーソルバーなしを採用」で確定。初回の原案からカーソルだけを除去し、三本線の位置・長さ・太さ、暗い角丸背景と白系の色は維持。

- `minimal-editor.svg`: 編集用原本（32 × 32の座標系）。
- `minimal-editor.png`: 256px、角の外側は透過。
- `minimal-editor.ico`: 16 / 20 / 24 / 32 / 40 / 48 / 64 / 128 / 256px。

再生成はリポジトリで `node scripts/build-icon.mjs`。インストール済みEdgeを非表示で使い、専用プロファイルを`.work/`へ作る。通常のビルドではブラウザを起動せず、追跡済みICOを使う。

`scripts/build-native.ps1`はコンパイル後の一時exeに`embed-icon.ps1`でWindowsのRT_ICON／RT_GROUP_ICONを組み込み、成功後に日常版へ配置する。SDKのリソースコンパイラは不要。リソースは他の既存リソースを残して追加する。

アプリはWM_GETICONのICON_BIGへDPIに合うアイコンを返す。ICON_SMALL／SMALL2とクラスアイコンは空にし、タイトルバーは従来どおり表示しない。スタートメニューは`install-shortcut.ps1`がICOを指定し、当該ショートカットの表示更新を通知する。
