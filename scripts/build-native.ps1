$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
New-Item -ItemType Directory -Force (Join-Path $repo 'build') | Out-Null
# One compiler at a time; no crate downloads, LTO, or debug-symbol generation.
$staging = Join-Path $repo "build/native-next-build-$PID.exe"
try {
    & rustc --edition=2021 -C opt-level=s -C debuginfo=0 -C codegen-units=1 (Join-Path $repo 'prototypes/native-next/main.rs') -o $staging
    if ($LASTEXITCODE -ne 0) { throw 'Native build failed; previous executable is intact' }
    Move-Item -LiteralPath $staging -Destination (Join-Path $repo 'build/native-next.exe') -Force
} finally {
    if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging }
}
