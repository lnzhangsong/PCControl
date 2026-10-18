' Start PC Remote Server silently (no console window).
' Used by the shortcut in the Startup folder for auto-start at logon.
Option Explicit

Dim fso, sh, baseDir, exePath
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh = CreateObject("WScript.Shell")

baseDir = fso.GetParentFolderName(WScript.ScriptFullName)
exePath = baseDir & "\pccontrol-server.exe"

If Not fso.FileExists(exePath) Then
    MsgBox "pccontrol-server.exe not found in:" & vbCrLf & baseDir, 16, "PC Remote Server"
    WScript.Quit 1
End If

sh.CurrentDirectory = baseDir
' 0 = hidden window, False = do not wait
sh.Run """" & exePath & """", 0, False
