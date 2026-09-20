# Oeffnet den Finanzhelfer so, dass er funktioniert:
# Bruecke laeuft? Wenn nicht, starten. Dann die App im eigenen Fenster oeffnen.
#
# Dieses Skript haengt hinter der Desktop-Verknuepfung (siehe verknuepfung.ps1).
# Von Hand geht es auch:  .\start-app.ps1

$ErrorActionPreference = "Stop"
$ordner = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $ordner "target\release\finanzhelfer-bruecke.exe"

# Adresse aus der config.toml lesen, statt sie hier noch einmal festzuschreiben
$adresse = "127.0.0.1:8123"
$configPfad = Join-Path $ordner "config.toml"
if (Test-Path $configPfad) {
    $treffer = Select-String -Path $configPfad -Pattern '^\s*adresse\s*=\s*"([^"]+)"' | Select-Object -First 1
    if ($treffer) { $adresse = $treffer.Matches[0].Groups[1].Value }
}
# 0.0.0.0 heisst "lausche ueberall" - ansprechen muss man sie trotzdem lokal
$url = "http://" + $adresse.Replace("0.0.0.0", "localhost")

function Erreichbar {
    try {
        Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 2 | Out-Null
        return $true
    } catch { return $false }
}

if (-not (Erreichbar)) {
    if (-not (Test-Path $exe)) {
        [System.Windows.Forms.MessageBox]::Show(
            "Die Bruecke ist noch nicht gebaut.`n`nIm Ordner $ordner einmal ausfuehren:`n  cargo build --release",
            "Finanzhelfer") | Out-Null
        exit 1
    }
    # Versteckt starten; die Aufgabe aus autostart.ps1 macht es genauso
    Start-Process -FilePath "conhost.exe" `
                  -ArgumentList "--headless", "`"$exe`"", "dienst" `
                  -WorkingDirectory $ordner

    # Der erste Start liest die Konfiguration und laedt den Bestand - kurz warten
    $bis = (Get-Date).AddSeconds(20)
    while (-not (Erreichbar) -and (Get-Date) -lt $bis) { Start-Sleep -Milliseconds 400 }
}

if (-not (Erreichbar)) {
    Add-Type -AssemblyName System.Windows.Forms
    [System.Windows.Forms.MessageBox]::Show(
        "Die Bruecke antwortet nicht unter $url.`n`nStand pruefen mit:`n  .\autostart.ps1 -Zeigen",
        "Finanzhelfer") | Out-Null
    exit 1
}

# Im App-Modus oeffnen: eigenes Fenster, keine Adresszeile, keine Tableiste.
# Faellt zurueck auf den Standardbrowser, wenn kein Chrome/Edge da ist.
$browser = @(
    "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
    "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe",
    "$env:LOCALAPPDATA\Google\Chrome\Application\chrome.exe",
    "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe",
    "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($browser) {
    Start-Process -FilePath $browser -ArgumentList "--app=$url"
} else {
    Start-Process $url
}
