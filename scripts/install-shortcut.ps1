$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$programs = [Environment]::GetFolderPath('Programs')
$shortcutPath = Join-Path $programs 'minimal-editor.lnk'
$previousPath = Join-Path $programs 'Minimal Editor.lnk'
$shell = New-Object -ComObject WScript.Shell
$target = Join-Path $env:WINDIR 'System32\wscript.exe'
$arguments = '"' + (Join-Path $repo 'scripts\app.vbs') + '"'
foreach ($candidate in @($shortcutPath, $previousPath)) {
    if (Test-Path -LiteralPath $candidate) {
        $existing = $shell.CreateShortcut($candidate)
        $expected = if ($candidate -eq $previousPath) { '"' + (Join-Path $repo 'scripts\launch.vbs') + '"' } else { $arguments }
        if ($existing.TargetPath -ne $target -or $existing.Arguments -ne $expected) {
            throw "Existing unrelated shortcut; refusing to change: $candidate"
        }
    }
}
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $target
$shortcut.Arguments = $arguments
$shortcut.WorkingDirectory = $repo
$shortcut.Description = 'Write independently. Saved automatically on this computer.'
$shortcut.Save()
if (Test-Path -LiteralPath $previousPath) { Remove-Item -LiteralPath $previousPath }
Write-Output $shortcutPath
