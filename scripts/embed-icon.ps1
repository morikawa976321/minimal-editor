param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$icon = [IO.File]::ReadAllBytes((Join-Path $repo 'assets/minimal-editor.ico'))
# The OS resource API works with either Rust linker; no SDK resource compiler.
if (-not ('MinimalEditorIconResources' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class MinimalEditorIconResources {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr BeginUpdateResourceW(string file, bool deleteExisting);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool UpdateResourceW(IntPtr update, IntPtr type, IntPtr name, ushort language, byte[] data, uint size);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool EndUpdateResourceW(IntPtr update, bool discard);
    public static void Embed(string file, byte[] ico) {
        if (ico.Length < 6 || BitConverter.ToUInt16(ico, 0) != 0 || BitConverter.ToUInt16(ico, 2) != 1)
            throw new ArgumentException("Invalid icon header");
        int count = BitConverter.ToUInt16(ico, 4);
        if (count < 1 || ico.Length < 6 + 16 * count) throw new ArgumentException("Invalid icon directory");
        byte[] group = new byte[6 + 14 * count];
        Buffer.BlockCopy(ico, 0, group, 0, 6);
        // Validate every frame before opening the executable for update.
        for (int i=0; i<count; i++) {
            int at=6+16*i;
            uint size=BitConverter.ToUInt32(ico, at+8), offset=BitConverter.ToUInt32(ico, at+12);
            if (size==0 || offset < 6+16*count || (ulong)offset+size > (ulong)ico.Length)
                throw new ArgumentException("Invalid icon frame");
            Buffer.BlockCopy(ico, at, group, 6+14*i, 12);
            Buffer.BlockCopy(BitConverter.GetBytes((ushort)(i+1)), 0, group, 6+14*i+12, 2);
        }
        IntPtr handle=BeginUpdateResourceW(file, false);
        if (handle==IntPtr.Zero) throw new Win32Exception();
        bool committed=false;
        try {
            for (int i=0; i<count; i++) {
                int at=6+16*i, size=checked((int)BitConverter.ToUInt32(ico, at+8));
                byte[] image=new byte[size];
                Buffer.BlockCopy(ico, checked((int)BitConverter.ToUInt32(ico, at+12)), image, 0, size);
                if (!UpdateResourceW(handle, (IntPtr)3, (IntPtr)(i+1), 0, image, (uint)size)) throw new Win32Exception();
            }
            if (!UpdateResourceW(handle, (IntPtr)14, (IntPtr)1, 0, group, (uint)group.Length)) throw new Win32Exception();
            bool result=EndUpdateResourceW(handle, false);
            handle=IntPtr.Zero;
            if (!result) throw new Win32Exception();
            committed=true;
        } finally {
            if (!committed && handle!=IntPtr.Zero) EndUpdateResourceW(handle, true);
        }
    }
}
'@
}
[MinimalEditorIconResources]::Embed([IO.Path]::GetFullPath($Executable), $icon)
