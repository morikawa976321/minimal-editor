$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$work = Join-Path $repo ('.work/font-preview-build-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $work | Out-Null
$previous400 = $env:MINIMAL_EDITOR_FONT_400_EXE
$previous500 = $env:MINIMAL_EDITOR_FONT_500_EXE
$previous700 = $env:MINIMAL_EDITOR_FONT_700_EXE
try {
    foreach ($weight in @(400, 500, 700)) {
        $source = Join-Path $work "$weight"
        New-Item -ItemType Directory $source | Out-Null
        Get-ChildItem -LiteralPath (Join-Path $repo 'prototypes/native-next') -Filter '*.rs' | Copy-Item -Destination $source
        $main = Join-Path $source 'main.rs'
        $code = [IO.File]::ReadAllText($main) -replace 'const FONT_WEIGHT: u16 = \d+;', "const FONT_WEIGHT: u16 = $weight;"
        [IO.File]::WriteAllText($main, $code)
        $exe = Join-Path $work "native-$weight.exe"
        & rustc --edition=2021 -C opt-level=s -C debuginfo=0 -C codegen-units=1 $main -o $exe
        if ($LASTEXITCODE -ne 0) { throw "Preview build failed: $weight" }
        if ($weight -eq 400) { $env:MINIMAL_EDITOR_FONT_400_EXE = $exe }
        elseif ($weight -eq 500) { $env:MINIMAL_EDITOR_FONT_500_EXE = $exe }
        else { $env:MINIMAL_EDITOR_FONT_700_EXE = $exe }
    }
    $python = Join-Path $env:APPDATA 'uv/python/cpython-3.12-windows-x86_64-none/python.exe'
    & $python (Join-Path $repo 'tests/native_font_preview.py')
    if ($LASTEXITCODE -ne 0) { throw 'Native font capture failed' }
} finally {
    $env:MINIMAL_EDITOR_FONT_400_EXE = $previous400
    $env:MINIMAL_EDITOR_FONT_500_EXE = $previous500
    $env:MINIMAL_EDITOR_FONT_700_EXE = $previous700
}
