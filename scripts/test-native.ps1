$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    New-Item -ItemType Directory -Force .work/tmp | Out-Null
    $env:TEMP = Join-Path $repo '.work/tmp'
    $env:TMP = $env:TEMP
    $env:PYTHONUTF8 = '1'
    $python = Join-Path $env:APPDATA 'uv/python/cpython-3.12-windows-x86_64-none/python.exe'
    & ./scripts/build-native.ps1
    & rustc --edition=2021 --test -C debuginfo=0 tests/native_worker.rs -o build/native-next-store-tests.exe
    if ($LASTEXITCODE -ne 0) { throw 'Native store tests did not build' }
    & ./build/native-next-store-tests.exe --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Native store tests failed' }
    & rustc --edition=2021 -C opt-level=s -C debuginfo=0 prototypes/native-next/adopt.rs -o build/native-adopt.exe
    if ($LASTEXITCODE -ne 0) { throw 'Native adoption tool did not build' }
    & rustc --edition=2021 --test -C debuginfo=0 prototypes/native-next/adopt.rs -o build/native-adopt-tests.exe
    if ($LASTEXITCODE -ne 0) { throw 'Native adoption tests did not build' }
    & ./build/native-adopt-tests.exe --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Native adoption tests failed' }
    & node --test tests/native-seed.test.mjs tests/native-comparison.test.mjs tests/native-launch.test.mjs tests/native-adopt.test.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Native launcher tests failed' }
    & $python tests/native_next_desktop.py
    if ($LASTEXITCODE -ne 0) { throw 'Private desktop native GUI tests failed' }
    & $python tests/native_font_desktop.py
    if ($LASTEXITCODE -ne 0) { throw 'Native per-character font tests failed' }
    & $python tests/native_picker_desktop.py
    if ($LASTEXITCODE -ne 0) { throw 'Native picker paint tests failed' }
    & $python tests/native_scroll_desktop.py
    if ($LASTEXITCODE -ne 0) { throw 'Native wheel and scrollbar tests failed' }
    & $python tests/native_async_desktop.py
    if ($LASTEXITCODE -ne 0) { throw 'Asynchronous native GUI tests failed' }
    & node tests/native_comparison_ui.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Headless comparison page tests failed' }
} finally { Pop-Location }
