param(
    [string]$ProfilePath = (Join-Path (Split-Path $PSScriptRoot -Parent) '.work\edge-profile'),
    [string[]]$Title = @('minimal-editor'),
    [switch]$ProbeOnly,
    [switch]$NoLegacyTitle
)
$ErrorActionPreference = 'Stop'

# Inspect only Edge processes; never return command lines (which can contain
# the local server token). The profile argument is parsed using Windows rules.
if (-not ('MinimalEditorWindow' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class MinimalEditorWindow {
    private delegate bool EnumProc(IntPtr hwnd, IntPtr data);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc callback, IntPtr data);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] private static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] private static extern bool ShowWindowAsync(IntPtr hwnd, int command);
    [DllImport("user32.dll")] private static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("shell32.dll", CharSet=CharSet.Unicode)] private static extern IntPtr CommandLineToArgvW(string command, out int count);
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr memory);
    public static string[] Arguments(string command) {
        int count;
        IntPtr memory = CommandLineToArgvW(command, out count);
        if (memory == IntPtr.Zero) throw new InvalidOperationException("Cannot parse Edge process arguments");
        try {
            string[] args = new string[count];
            for (int i=0; i<count; i++) args[i] = Marshal.PtrToStringUni(Marshal.ReadIntPtr(memory, i*IntPtr.Size));
            return args;
        } finally { LocalFree(memory); }
    }
    public static long Find(int[] processIds, string[] titles, bool legacy) {
        var allowed = new HashSet<int>(processIds);
        long found = 0;
        EnumWindows(delegate(IntPtr hwnd, IntPtr unused) {
            uint pid;
            GetWindowThreadProcessId(hwnd, out pid);
            if (!allowed.Contains((int)pid) || !IsWindowVisible(hwnd)) return true;
            var name = new StringBuilder(512);
            GetClassName(hwnd, name, name.Capacity);
            if (name.ToString() != "Chrome_WidgetWin_1") return true;
            name.Clear(); GetWindowText(hwnd, name, name.Capacity);
            string title = name.ToString();
            if (title.EndsWith(" - Microsoft Edge", StringComparison.Ordinal)) title = title.Substring(0, title.Length-17);
            bool matches = Array.IndexOf(titles, title) >= 0;
            // Previous V0 used these titles, including during an unsaved edit.
            if (legacy && (title == "A \u00b7 minimal editor" || title.EndsWith(" \u00b7 A", StringComparison.Ordinal))) matches = true;
            if (!matches) return true;
            found = hwnd.ToInt64(); return false;
        }, IntPtr.Zero);
        return found;
    }
    public static bool Activate(long handle) {
        IntPtr hwnd = new IntPtr(handle);
        if (IsIconic(hwnd)) ShowWindowAsync(hwnd, 9);
        return SetForegroundWindow(hwnd);
    }
}
'@
}

$wantedProfile = [IO.Path]::GetFullPath($ProfilePath).TrimEnd('\', '/')
$ownedProcessIds = @(
    foreach ($edgeProcess in Get-CimInstance Win32_Process -Filter "Name = 'msedge.exe'") {
        if (-not $edgeProcess.CommandLine) { continue }
        $arguments = [MinimalEditorWindow]::Arguments($edgeProcess.CommandLine)
        for ($index = 1; $index -lt $arguments.Length; $index++) {
            $candidate = $null
            if ($arguments[$index].StartsWith('--user-data-dir=', [StringComparison]::Ordinal)) {
                $candidate = $arguments[$index].Substring(16)
            } elseif ($arguments[$index] -eq '--user-data-dir' -and $index + 1 -lt $arguments.Length) {
                $candidate = $arguments[$index + 1]
            }
            if ($candidate -and [IO.Path]::GetFullPath($candidate).TrimEnd('\', '/') -eq $wantedProfile) {
                [int]$edgeProcess.ProcessId
                break
            }
        }
    }
)
$handle = [MinimalEditorWindow]::Find([int[]]$ownedProcessIds, $Title, -not $NoLegacyTitle)
$focused = $false
if ($handle -ne 0 -and -not $ProbeOnly) { $focused = [MinimalEditorWindow]::Activate($handle) }
# A found window still counts if Windows declines foreground activation. Never
# create a duplicate solely because another application currently has focus.
@{ found = ($handle -ne 0); handle = $handle; focused = $focused; probeOnly = [bool]$ProbeOnly } | ConvertTo-Json -Compress
