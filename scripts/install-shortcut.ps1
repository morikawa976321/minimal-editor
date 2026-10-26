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
$shortcut.IconLocation = (Join-Path $repo 'assets/minimal-editor.ico') + ',0'
$shortcut.Description = 'Write independently. Saved automatically on this computer.'
$shortcut.Save()
if (-not ('MinimalEditorShortcutNotify' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class MinimalEditorShortcutNotify {
    [DllImport("shell32.dll", CharSet=CharSet.Unicode)]
    public static extern void SHChangeNotify(uint change, uint flags, string item, IntPtr other);
}
'@
}
[MinimalEditorShortcutNotify]::SHChangeNotify(0x2000, 0x1005, $shortcutPath, [IntPtr]::Zero)
if (Test-Path -LiteralPath $previousPath) { Remove-Item -LiteralPath $previousPath }
Write-Output $shortcutPath
