$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
New-Item -ItemType Directory -Force (Join-Path $repo 'build') | Out-Null
# One compiler at a time; no crate downloads, LTO, or debug-symbol generation.
$identity = [IO.Path]::GetFullPath($repo).TrimEnd('\', '/').ToLowerInvariant()
$hasher = [Security.Cryptography.SHA256]::Create()
try { $digest = [BitConverter]::ToString($hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($identity))).Replace('-', '') }
finally { $hasher.Dispose() }
$mutex = New-Object Threading.Mutex($false, ('Local\minimal-editor-native-build-' + $digest))
$acquired = $false
$token = [Guid]::NewGuid().ToString('N')
$staging = Join-Path $repo "build/native-next-build-$token.exe"
$canonical = Join-Path $repo 'build/native-next.exe'
$retired = Join-Path $repo "build/native-next-retired-$token.exe"
$retiredPrevious = $false
try {
    try { $acquired = $mutex.WaitOne(30000) }
    catch [Threading.AbandonedMutexException] { $acquired = $true }
    if (-not $acquired) { throw 'Another native build is still in progress; previous executable is intact' }
    & rustc --edition=2021 -C opt-level=s -C debuginfo=0 -C codegen-units=1 (Join-Path $repo 'prototypes/native-next/main.rs') -o $staging
    if ($LASTEXITCODE -ne 0) { throw 'Native build failed; previous executable is intact' }
    # Windows allows an executing image to be renamed, but not overwritten.
    # Keep that image on disk for the running process and possible rollback.
    if (Test-Path -LiteralPath $canonical) {
        Move-Item -LiteralPath $canonical -Destination $retired
        $retiredPrevious = $true
    }
    try {
        Move-Item -LiteralPath $staging -Destination $canonical
    } catch {
        $publishError = $_
        if ($retiredPrevious -and -not (Test-Path -LiteralPath $canonical)) {
            try { Move-Item -LiteralPath $retired -Destination $canonical }
            catch { throw "Native publication and rollback both failed. Previous executable remains at $retired. $publishError $_" }
        }
        throw $publishError
    }
} finally {
    try {
        if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging }
    } finally {
        if ($acquired) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
}
