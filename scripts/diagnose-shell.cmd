@echo off
setlocal EnableExtensions
set "OUT=%USERPROFILE%\Desktop\shell-diagnose.txt"
set "TS=%DATE% %TIME%"

> "%OUT%" echo === Shell / PATH diagnose ===
>>"%OUT%" echo Time: %TS%
>>"%OUT%" echo.

>>"%OUT%" echo --- HKCU Path ---
reg query "HKCU\Environment" /v Path >>"%OUT%" 2>&1

>>"%OUT%" echo.
>>"%OUT%" echo --- HKLM Path ---
reg query "HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment" /v Path >>"%OUT%" 2>&1

>>"%OUT%" echo.
>>"%OUT%" echo --- where pwsh ---
where pwsh >>"%OUT%" 2>&1

>>"%OUT%" echo.
>>"%OUT%" echo --- conda.exe ---
if exist "%USERPROFILE%\anaconda3\Scripts\conda.exe" (
    >>"%OUT%" echo EXISTS: %USERPROFILE%\anaconda3\Scripts\conda.exe
) else (
    >>"%OUT%" echo MISSING: %USERPROFILE%\anaconda3\Scripts\conda.exe
)

>>"%OUT%" echo.
>>"%OUT%" echo --- pwsh via PATH alias ---
pwsh -NoProfile -NoLogo -Command "Write-Output alias-ok" >>"%OUT%" 2>&1
if errorlevel 1 >>"%OUT%" echo pwsh alias failed with error %ERRORLEVEL%

>>"%OUT%" echo.
>>"%OUT%" echo --- pwsh via Store package path ---
set "PWSH_PKG=C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.2.0_x64__8wekyb3d8bbwe\pwsh.exe"
if exist "%PWSH_PKG%" (
    "%PWSH_PKG%" -NoProfile -NoLogo -Command "Write-Output direct-ok" >>"%OUT%" 2>&1
) else (
    >>"%OUT%" echo MISSING: %PWSH_PKG%
)

>>"%OUT%" echo.
>>"%OUT%" echo --- powershell.exe -NoProfile ---
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NoLogo -Command "Write-Output ok" >>"%OUT%" 2>&1

>>"%OUT%" echo.
>>"%OUT%" echo === Done. Paste this file in chat if you need help parsing PATH. ===

notepad "%OUT%"
