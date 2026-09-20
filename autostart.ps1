# Richtet die Bruecke als Aufgabe ein, die bei der Anmeldung von selbst startet.
#
#   .\autostart.ps1              einrichten
#   .\autostart.ps1 -Entfernen   wieder abschalten
#   .\autostart.ps1 -Zeigen      Stand anzeigen
#
# Braucht keine Administratorrechte: die Aufgabe laeuft unter dem eigenen
# Konto und nur, wenn man angemeldet ist. Das genuegt - ohne Anmeldung ist
# auch niemand da, der die App benutzt.

param(
    [switch]$Entfernen,
    [switch]$Zeigen
)

$ErrorActionPreference = "Stop"
$name = "Finanzhelfer-Bruecke"
$ordner = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $ordner "target\release\finanzhelfer-bruecke.exe"

function Zustand {
    $a = Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
    if (-not $a) { return $null }
    $a
}

if ($Zeigen) {
    $a = Zustand
    if (-not $a) {
        "Nicht eingerichtet."
    } else {
        $info = Get-ScheduledTaskInfo -TaskName $name
        "Aufgabe        : $($a.TaskName)"
        "Zustand        : $($a.State)"
        "Zuletzt        : $($info.LastRunTime)  (Ergebnis $($info.LastTaskResult))"
        "Programm       : $($a.Actions[0].Execute)"
    }
    $laeuft = Get-Process -Name "finanzhelfer-bruecke" -ErrorAction SilentlyContinue
    if ($laeuft) { "Laeuft gerade  : PID $($laeuft.Id)" } else { "Laeuft gerade  : nein" }
    return
}

if ($Entfernen) {
    if (Zustand) {
        Unregister-ScheduledTask -TaskName $name -Confirm:$false
        "Autostart entfernt. Die Bruecke laeuft weiter, bis du sie beendest."
    } else {
        "War nicht eingerichtet."
    }
    return
}

if (-not (Test-Path $exe)) {
    throw "$exe fehlt. Erst bauen: cargo build --release"
}
if (-not (Test-Path (Join-Path $ordner "config.toml"))) {
    throw "config.toml fehlt neben dem Skript."
}

# -WindowStyle Hidden am Programm selbst hilft bei einer Konsolenanwendung
# nicht - deshalb startet die Aufgabe ueber conhost mit versteckter Konsole.
$aktion = New-ScheduledTaskAction `
    -Execute "conhost.exe" `
    -Argument "--headless `"$exe`" dienst" `
    -WorkingDirectory $ordner

$ausloeser = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME

# Netzwerk kann bei der Anmeldung noch fehlen; die Bruecke haelt das aus und
# versucht es beim naechsten Durchlauf wieder. Kein Abbruch nach Zeit, kein
# Stoppen im Akkubetrieb - sonst steht sie am Laptop dauernd still.
$einstellungen = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -StartWhenAvailable `
    -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -RestartCount 3 `
    -RestartInterval (New-TimeSpan -Minutes 5)

if (Zustand) { Unregister-ScheduledTask -TaskName $name -Confirm:$false }

Register-ScheduledTask `
    -TaskName $name `
    -Action $aktion `
    -Trigger $ausloeser `
    -Settings $einstellungen `
    -Description "Holt Kontostaende, Umsaetze und Depotbestaende fuer den Finanzhelfer." | Out-Null

"Eingerichtet. Die Bruecke startet ab jetzt bei jeder Anmeldung."
""
"  Jetzt starten   : Start-ScheduledTask -TaskName '$name'"
"  Stand ansehen   : .\autostart.ps1 -Zeigen"
"  Wieder abschalten: .\autostart.ps1 -Entfernen"
