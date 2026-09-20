# Legt eine Verknuepfung "Finanzhelfer" auf den Desktop, die die App im
# eigenen Fenster oeffnet und die Bruecke bei Bedarf vorher startet.
#
#   .\verknuepfung.ps1              anlegen
#   .\verknuepfung.ps1 -Entfernen   wieder loeschen
#
# Das Symbol wird aus icon-512.png der App erzeugt - Windows-Verknuepfungen
# brauchen eine .ico, PNG allein genuegt nicht.

param([switch]$Entfernen)

$ErrorActionPreference = "Stop"
$ordner = Split-Path -Parent $MyInvocation.MyCommand.Path
$desktop = [Environment]::GetFolderPath("Desktop")
$lnk = Join-Path $desktop "Finanzhelfer.lnk"

if ($Entfernen) {
    if (Test-Path $lnk) { Remove-Item $lnk; "Verknuepfung entfernt." } else { "War keine da." }
    return
}

# ---- Symbol erzeugen ----
$ico = Join-Path $ordner "finanzhelfer.ico"
$png = Join-Path $ordner "..\finanzhelfer\icon-512.png"

if ((Test-Path $png) -and -not (Test-Path $ico)) {
    Add-Type -AssemblyName System.Drawing
    $quelle = [System.Drawing.Image]::FromFile((Resolve-Path $png))
    # Auf 256 bringen: groesser laesst das ICO-Format nicht zu
    $klein = New-Object System.Drawing.Bitmap 256, 256
    $g = [System.Drawing.Graphics]::FromImage($klein)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.DrawImage($quelle, 0, 0, 256, 256)
    $g.Dispose(); $quelle.Dispose()

    $puffer = New-Object System.IO.MemoryStream
    $klein.Save($puffer, [System.Drawing.Imaging.ImageFormat]::Png)
    $klein.Dispose()
    $pngBytes = $puffer.ToArray()
    $puffer.Dispose()

    # ICO von Hand zusammensetzen. Seit Vista darf ein Eintrag ein ganzes PNG
    # sein - das spart das Umrechnen in eine Bitmap mit Maske.
    $datei = [System.IO.File]::Create($ico)
    $s = New-Object System.IO.BinaryWriter($datei)
    $s.Write([UInt16]0); $s.Write([UInt16]1); $s.Write([UInt16]1)   # Kopf: Typ 1, ein Bild
    $s.Write([Byte]0); $s.Write([Byte]0)                            # 0 = 256 Pixel
    $s.Write([Byte]0); $s.Write([Byte]0)                            # Palette, reserviert
    $s.Write([UInt16]1); $s.Write([UInt16]32)                       # Ebenen, Bit je Pixel
    $s.Write([UInt32]$pngBytes.Length)
    $s.Write([UInt32]22)                                            # Beginn der Bilddaten
    $s.Write($pngBytes)
    $s.Flush(); $s.Close(); $datei.Close()
    "Symbol erzeugt: $ico"
}

# ---- Verknuepfung anlegen ----
$skript = Join-Path $ordner "start-app.ps1"
if (-not (Test-Path $skript)) { throw "start-app.ps1 fehlt neben diesem Skript." }

$shell = New-Object -ComObject WScript.Shell
$v = $shell.CreateShortcut($lnk)
# -WindowStyle Hidden, damit beim Klicken kein blaues Fenster aufblitzt
$v.TargetPath = "powershell.exe"
$v.Arguments = "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$skript`""
$v.WorkingDirectory = $ordner
$v.Description = "Finanzhelfer oeffnen (startet bei Bedarf die Bruecke)"
if (Test-Path $ico) { $v.IconLocation = "$ico,0" }
$v.Save()

"Verknuepfung liegt auf dem Desktop: $lnk"
"Ein Doppelklick startet die Bruecke, falls noetig, und oeffnet die App im eigenen Fenster."
