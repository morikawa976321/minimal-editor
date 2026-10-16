$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    New-Item -ItemType Directory -Force .work/tmp | Out-Null
    $env:TEMP = Join-Path $repo '.work/tmp'
    $env:TMP = $env:TEMP
    $env:PYTHONUTF8 = '1'
    $python = Join-Path $env:APPDATA 'uv/python/cpython-3.12-windows-x86_64-none/python.exe'
    & $python -m unittest discover -s tests -p 'test_tk*.py' -v
    if ($LASTEXITCODE -ne 0) { throw 'Tk tests failed' }
    & node --test tests/server.test.mjs tests/memos.test.mjs tests/server-upgrade.test.mjs
    if ($LASTEXITCODE -ne 0) { throw 'HTTP tests failed' }
    & ./tests/native.ps1
    & node tests/gui-launch.mjs
    if ($LASTEXITCODE -ne 0) { throw 'GUI launch test failed' }
    & $python tests/gui_desktop.py
    if ($LASTEXITCODE -ne 0) { throw 'Isolated desktop GUI test failed' }
    & $python tests/edge_desktop.py
    if ($LASTEXITCODE -ne 0) { throw 'Isolated desktop Edge launcher test failed' }
    & node tests/web-ui.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Headless Edge tests failed' }
} finally { Pop-Location }
