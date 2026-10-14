Set shell = CreateObject("WScript.Shell")
Set files = CreateObject("Scripting.FileSystemObject")
repo = files.GetParentFolderName(files.GetParentFolderName(WScript.ScriptFullName))
shell.Run "node.exe """ & repo & "\scripts\bootstrap.mjs""", 0, False
