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
    & rustc --edition=2021 --test -C debuginfo=0 prototypes/native-next/store.rs -o build/native-next-store-tests.exe
    if ($LASTEXITCODE -ne 0) { throw 'Native store tests did not build' }
    & ./build/native-next-store-tests.exe --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Native store tests failed' }
    & node --test tests/native-seed.test.mjs tests/native-comparison.test.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Native launcher tests failed' }
    & $python tests/native_next_desktop.py
    if ($LASTEXITCODE -ne 0) { throw 'Private desktop native GUI tests failed' }
    & node tests/native_comparison_ui.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Headless comparison page tests failed' }
} finally { Pop-Location }
