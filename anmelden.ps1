# Anmeldung bei einer Quelle erneuern.
#
#   .\anmelden.ps1            fragt, welches Konto
#   .\anmelden.ps1 tr         direkt Trade Republic
#   .\anmelden.ps1 -Liste     zeigt nur, was es gibt
#
# Warum es dieses Skript gibt: `anmelden tr` ist kein eigener Befehl, sondern
# ein Argument fuer finanzhelfer-bruecke.exe - und die liegt nicht im PATH.
# Wer das in ein frisches PowerShell-Fenster tippt, bekommt nur
# "CommandNotFoundException".
#
# Das MUSS ein eigenes Fenster sein: die Anmeldung fragt nach PIN und Code.
# Im Hintergrunddienst gibt es keine Tastatur - genau daran stirbt pytr mit
# "EOFError: EOF when reading a line", wenn die Sitzung abgelaufen ist.

param([string]$Konto, [switch]$Liste)

$ErrorActionPreference = "Stop"
$ordner = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $ordner "target\release\finanzhelfer-bruecke.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $ordner "finanzhelfer-bruecke.exe" }
if (-not (Test-Path $exe)) {
    Write-Host "finanzhelfer-bruecke.exe nicht gefunden. Erst `cargo build --release`." -ForegroundColor Red
    return
}

# Konten aus der config.toml lesen - id und name stehen dort je Block untereinander
$konten = @()
$id = $null
foreach ($zeile in (Get-Content (Join-Path $ordner "config.toml"))) {
    if ($zeile -match '^\s*id\s*=\s*"([^"]+)"') { $id = $Matches[1]; continue }
    if ($id -and $zeile -match '^\s*name\s*=\s*"([^"]+)"') {
        $konten += [pscustomobject]@{ Id = $id; Name = $Matches[1] }
        $id = $null
    }
}

if ($Liste -or -not $Konto) {
    Write-Host ""
    Write-Host "  Welche Quelle soll sich neu anmelden?" -ForegroundColor Cyan
    for ($i = 0; $i -lt $konten.Count; $i++) {
        "    [{0}]  {1,-22} {2}" -f ($i + 1), $konten[$i].Id, $konten[$i].Name | Write-Host
    }
    Write-Host ""
    if ($Liste) { return }
    $wahl = Read-Host "  Nummer (oder Enter zum Abbrechen)"
    if (-not $wahl) { return }
    $n = 0
    if (-not [int]::TryParse($wahl, [ref]$n) -or $n -lt 1 -or $n -gt $konten.Count) {
        Write-Host "  Das war keine gueltige Nummer." -ForegroundColor Red
        return
    }
    $Konto = $konten[$n - 1].Id
}

if ($konten.Id -notcontains $Konto) {
    Write-Host ("  '{0}' steht nicht in der config.toml. Bekannt: {1}" -f $Konto, ($konten.Id -join ", ")) -ForegroundColor Red
    return
}

Write-Host ""
Write-Host ("  Melde $Konto an. PIN und Code tippst du gleich selbst ein.") -ForegroundColor Cyan
Write-Host "  Geht es schief: ein paar Stunden warten, nicht sofort noch einmal -" -ForegroundColor DarkGray
Write-Host "  nach mehreren Fehlversuchen sperren die Banken fuer Stunden." -ForegroundColor DarkGray
Write-Host ""

& $exe anmelden $Konto

Write-Host ""
if ($LASTEXITCODE -eq 0) {
    Write-Host "  Fertig. Der Dienst holt beim naechsten Lauf wieder ab," -ForegroundColor Green
    Write-Host "  oder du stoesst es in der App unter 'Konten abrufen' an." -ForegroundColor Green
} else {
    Write-Host ("  Abgebrochen (Code {0}). Oben steht, woran es lag." -f $LASTEXITCODE) -ForegroundColor Red
}
Write-Host ""
