param([Parameter(Mandatory=$true)][int]$ServerPid)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$focusScript = Join-Path $PSScriptRoot 'focus-editor.ps1'
function Assert-EditorClosed {
    $window = & $focusScript -ProbeOnly | ConvertFrom-Json
    if ($window.found) { throw 'Close minimal-editor and reopen it to finish updating. The running editor was left unchanged.' }
}
Assert-EditorClosed
# Hold a process handle before inspecting CIM metadata so PID reuse cannot
# cause a later Kill() to target a different process.
$serverProcess = [Diagnostics.Process]::GetProcessById($ServerPid)
try {
    $pinnedHandle = $serverProcess.Handle
    $metadata = Get-CimInstance Win32_Process -Filter "ProcessId = $ServerPid"
    if (-not $metadata -or $metadata.Name -ne 'node.exe' -or -not $metadata.CommandLine) {
        throw 'Refusing to stop an unrecognized server process.'
    }
    $arguments = [MinimalEditorWindow]::Arguments($metadata.CommandLine)
    $expected = [IO.Path]::GetFullPath((Join-Path $repo 'scripts\server.mjs'))
    if ($arguments.Length -ne 2 -or -not [IO.Path]::IsPathRooted($arguments[1]) -or [IO.Path]::GetFullPath($arguments[1]) -ne $expected) {
        throw 'Refusing to stop a server outside this repository.'
    }
    if ($serverProcess.HasExited) { return }
    Assert-EditorClosed
    $serverProcess.Kill()
    if (-not $serverProcess.WaitForExit(5000)) { throw 'The previous editor server did not stop.' }
} finally { $serverProcess.Dispose() }
