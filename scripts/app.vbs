Set shell = CreateObject("WScript.Shell")
Set files = CreateObject("Scripting.FileSystemObject")
repo = files.GetParentFolderName(files.GetParentFolderName(WScript.ScriptFullName))
result = shell.Run("wscript.exe """ & repo & "\scripts\native-app.vbs""", 0, True)
WScript.Quit result
