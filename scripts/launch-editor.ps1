param(
    [string]$BootstrapScript = (Join-Path $PSScriptRoot 'bootstrap.mjs'),
    [string]$ProfilePath = (Join-Path (Split-Path $PSScriptRoot -Parent) '.work\edge-profile'),
    [string]$Title = 'minimal-editor',
    [switch]$NoLegacyTitle
)
$ErrorActionPreference = 'Stop'
$profileIdentity = [IO.Path]::GetFullPath($ProfilePath).TrimEnd('\', '/').ToLowerInvariant()
$hasher = [Security.Cryptography.SHA256]::Create()
try { $digest = [BitConverter]::ToString($hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($profileIdentity))).Replace('-', '') }
finally { $hasher.Dispose() }
$mutex = New-Object Threading.Mutex($false, ('Local\minimal-editor-launch-' + $digest))
$acquired = $false
$focusScript = Join-Path $PSScriptRoot 'focus-editor.ps1'
try {
    try { $acquired = $mutex.WaitOne(20000) }
    catch [Threading.AbandonedMutexException] { $acquired = $true }
    if (-not $acquired) { throw 'Another editor launch is still in progress.' }
    $existing = & $focusScript -ProfilePath $ProfilePath -Title $Title -NoLegacyTitle:$NoLegacyTitle | ConvertFrom-Json
    if ($existing.found) {
        @{ reused = $true; handle = $existing.handle; focused = $existing.focused } | ConvertTo-Json -Compress
        return
    }
    & node.exe $BootstrapScript
    if ($LASTEXITCODE -ne 0) { throw 'Editor startup failed; see .work/server.log.' }
    # Hold the launch lock until the new window can be found by the next launch.
    # Detect only here: actual foreground activation occurs on explicit reopens.
    $until = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $opened = & $focusScript -ProfilePath $ProfilePath -Title $Title -NoLegacyTitle:$NoLegacyTitle -ProbeOnly | ConvertFrom-Json
        if ($opened.found) {
            @{ reused = $false; handle = $opened.handle; focused = $false } | ConvertTo-Json -Compress
            return
        }
        Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $until)
    throw 'The editor window did not open in time.'
} catch {
    $logDirectory = Join-Path (Split-Path $PSScriptRoot -Parent) '.work'
    [IO.Directory]::CreateDirectory($logDirectory) | Out-Null
    ('{0:o} {1}' -f [DateTime]::UtcNow, ($_ | Out-String)) | Add-Content -LiteralPath (Join-Path $logDirectory 'launch.log') -Encoding UTF8
    throw
} finally {
    if ($acquired) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}
