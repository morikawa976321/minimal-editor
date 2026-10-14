$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$programs = [Environment]::GetFolderPath('Programs')
$shortcutPath = Join-Path $programs 'Minimal Editor.lnk'
if (Test-Path -LiteralPath $shortcutPath) {
    $existing = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcutPath)
    if ($existing.Arguments -ne ('"' + (Join-Path $repo 'scripts\launch.vbs') + '"')) { throw 'Existing unrelated shortcut; refusing to overwrite.' }
}
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcutPath)
$shortcut.TargetPath = Join-Path $env:WINDIR 'System32\wscript.exe'
$shortcut.Arguments = '"' + (Join-Path $repo 'scripts\launch.vbs') + '"'
$shortcut.WorkingDirectory = $repo
$shortcut.Description = 'Minimal Editor V0 comparison'
$shortcut.Save()
Write-Output $shortcutPath
