@echo off
rem Anmeldung bei einer Quelle erneuern - per Doppelklick oder aus der Konsole.
rem
rem   anmelden.cmd        fragt, welches Konto
rem   anmelden.cmd tr     direkt Trade Republic
rem
rem Warum eine .cmd neben der .ps1: auf einem frischen Windows ist die
rem Ausfuehrung von PowerShell-Skripten gesperrt ("about_Execution_Policies"),
rem und .\anmelden.ps1 laeuft dann in einen PSSecurityException. Eine .cmd
rem faellt nicht unter diese Sperre; sie startet die .ps1 mit -ExecutionPolicy
rem Bypass, was nur fuer diesen einen Aufruf gilt und nichts am System aendert.

setlocal
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0anmelden.ps1" %*
echo.
pause
