$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
New-Item -ItemType Directory -Force (Join-Path $repo 'build') | Out-Null
# One compiler at a time; no crate downloads, LTO, or debug-symbol generation.
& rustc --edition=2021 -C opt-level=s -C debuginfo=0 -C codegen-units=1 (Join-Path $repo 'prototypes/native-next/main.rs') -o (Join-Path $repo 'build/native-next.exe')
if ($LASTEXITCODE -ne 0) { throw 'Native comparison build failed' }
