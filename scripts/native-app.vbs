Set shell = CreateObject("WScript.Shell")
Set files = CreateObject("Scripting.FileSystemObject")
repo = files.GetParentFolderName(files.GetParentFolderName(WScript.ScriptFullName))
native = repo & "\build\native-next.exe"
catalog = repo & "\data\native-next\catalog.tsv"
If files.FileExists(native) And files.FileExists(catalog) Then
    shell.Run """" & native & """ --data-dir """ & repo & "\data\native-next""", 1, False
    WScript.Quit 0
End If
result = shell.Run("node.exe """ & repo & "\scripts\launch-native.mjs""", 0, True)
If result <> 0 Then
    MsgBox "Native editor could not start. See .work\native-launch.log in " & repo, 16, "minimal-editor Native"
End If
WScript.Quit result
