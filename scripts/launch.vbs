Set shell = CreateObject("WScript.Shell")
Set files = CreateObject("Scripting.FileSystemObject")
repo = files.GetParentFolderName(files.GetParentFolderName(WScript.ScriptFullName))
shell.Run "node.exe """ & repo & "\scripts\bootstrap.mjs"" --compare", 0, False
