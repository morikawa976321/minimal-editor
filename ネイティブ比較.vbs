Set shell = CreateObject("WScript.Shell")
Set files = CreateObject("Scripting.FileSystemObject")
repo = files.GetParentFolderName(WScript.ScriptFullName)
shell.Run "node.exe """ & repo & "\scripts\native-comparison.mjs""", 0, False
