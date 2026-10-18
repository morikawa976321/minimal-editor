Set shell = CreateObject("WScript.Shell")
Set files = CreateObject("Scripting.FileSystemObject")
repo = files.GetParentFolderName(files.GetParentFolderName(WScript.ScriptFullName))
result = shell.Run("powershell.exe -NoProfile -ExecutionPolicy Bypass -File """ & repo & "\scripts\launch-editor.ps1""", 0, True)
If result <> 0 Then
    MsgBox "The browser editor could not start. See .work\launch.log and .work\server.log in " & repo, 16, "minimal-editor Browser"
End If
WScript.Quit result
