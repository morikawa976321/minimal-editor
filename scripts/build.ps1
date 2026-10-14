$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
New-Item -ItemType Directory -Force (Join-Path $repo 'build') | Out-Null
& rustc --edition=2021 -O (Join-Path $repo 'prototypes/native/main.rs') -o (Join-Path $repo 'build/native.exe')
if ($LASTEXITCODE -ne 0) { throw 'Native build failed' }
