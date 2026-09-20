# Finanzhelfer-Brücke

Der Teil des [Finanzhelfers](https://github.com/rianvegeta1991/finanzhelfer), der
wirklich mit den Banken spricht. Ein kleines Programm auf dem eigenen Rechner –
kein Server im Netz, kein Aggregator, kein Abo.

## Warum es das überhaupt gibt

Eine Seite, die im Browser läuft, kann Bankkonten nicht selbst abrufen:

- **FinTS/HBCI** ist kein Browser-Protokoll.
- **Open-Banking-Aggregatoren** verlangen ein geheimes Client-Secret und erlauben
  keine Zugriffe aus fremden Seiten. Ein Secret in einer öffentlichen Seite ist
  veröffentlicht. Die früher kostenlose Nordigen-Schnittstelle gibt es seit 2025
  nicht mehr; was bleibt, kostet Geld.
- **PSD2-Direktzugriff** braucht eine BaFin-Zulassung.

Also läuft der Bankteil dort, wo er hingehört: auf deinem Rechner. Die Brücke hält
der App genau drei Endpunkte hin, mehr weiß die App nicht von den Banken.

## Was angebunden ist

| Quelle | Wie | Was kommt an |
|---|---|---|
| **ING, Commerzbank, Sparkassen, Volksbanken …** | FinTS 3.0 über [`fints-rs`](https://crates.io/crates/fints-rs) | Saldo, Umsätze, Depotbestände |
| **Bitvavo** | offizielle REST-API, Schlüssel mit Leserecht | Krypto-Bestände und Kurse |
| **Trade Republic** | [`pytr`](https://github.com/MartinScharrer/pytr) – inoffiziell | Buchungen und Depotbestand |
| **FNZ Bank (ebase)** | — | kein FinTS-Zugang; bleibt Handarbeit |

Ob deine Bank FinTS kann, sagt dir `finanzhelfer-bruecke pruefen`: die Brücke
schlägt die Bankleitzahl im FinTS-Verzeichnis nach und zeigt den Endpunkt.

### Zu Trade Republic

TR hat keine offizielle Schnittstelle und kein FinTS. `pytr` baut das interne
Protokoll nach. Zwei Dinge dazu, offen gesagt:

1. **TRs Nutzungsbedingungen sehen automatisierten Zugriff nicht vor.** Es sind
   deine eigenen Daten aus deinem eigenen Konto, aber das Risiko liegt bei dir.
2. **Es kann jederzeit brechen.** Ändert TR sein Protokoll, steht der Abruf,
   bis `pytr` nachzieht.

Deshalb wird das Protokoll hier nicht selbst nachgebaut: die Brücke ruft `pytr`
auf und übersetzt nur dessen Ausgabe. Der zerbrechliche Teil bleibt bei einem
Werkzeug, das viele Leute gemeinsam pflegen.

## Einrichten

### 1. Produkt-ID beantragen (dauert am längsten – also zuerst)

Die Deutsche Kreditwirtschaft verlangt, dass sich FinTS-Programme ausweisen.
Die Registrierung ist kostenlos, aber sie dauert ein paar Tage:
<https://www.hbci-zka.de/register/prod_register.htm>

Ohne Produkt-ID weisen die meisten Banken den Zugang ab.

### 2. Bauen

```powershell
cargo build --release
```

Die fertige Datei liegt danach unter `target\release\finanzhelfer-bruecke.exe`.

### 3. Konfigurieren

`config.beispiel.toml` nach `config.toml` kopieren und ausfüllen. Die Datei
enthält Zugangsdaten und steht deshalb in der `.gitignore`.

Wer die PIN nicht im Klartext ablegen will, lässt das Feld leer und setzt
stattdessen eine Umgebungsvariable:

```powershell
$env:FH_PIN_ING_GIRO = "1234"
```

### 4. Einmalig anmelden

```powershell
.\finanzhelfer-bruecke.exe anmelden ing-giro
```

Die Bank schickt eine Freigabe in deine Banking-App. Bestätige sie; die Brücke
merkt sich danach die System-Kennung und kommt meist ~90 Tage ohne weitere TAN
aus. Verlangt die Bank wieder eine, sagt der Dienst Bescheid – dann denselben
Befehl noch einmal.

Für Trade Republic stattdessen einmalig:

```powershell
python -m pip install pytr
python -m pytr login --store_credentials
```

### 5. Laufen lassen

```powershell
.\finanzhelfer-bruecke.exe dienst
```

In der App unter **Mehr → Automatischer Abruf** die Adresse
(`http://localhost:8123`) und das Token aus der `config.toml` eintragen, auf
*Verbindung prüfen* tippen – dort stehen dann die gefundenen Kennungen. Die
trägst du beim jeweiligen Konto bzw. Depot unter „Kennung bei der Brücke" ein.

## Befehle

```
finanzhelfer-bruecke pruefen           Konfiguration und Bankzugänge anzeigen
finanzhelfer-bruecke anmelden <konto>  Einmalige Anmeldung mit TAN-Freigabe
finanzhelfer-bruecke abgleich [konto]  Einmal abrufen (alle oder eines)
finanzhelfer-bruecke dienst            Server starten (Voreinstellung)
```

## Die Schnittstelle

```
GET /konten                              → [{ref, name, bank, iban, art, saldo, waehrung}]
GET /umsaetze?konto=<ref>&von=YYYY-MM-DD → [{datum, betrag, gegen, zweck, waehrung}]
GET /positionen?depot=<ref>              → [{name, isin, wkn, symbol, art, stueck, einstand, kurs, waehrung}]
GET /status                              → Stand je Konto, für die Fehlersuche
```

Alle verlangen `Authorization: Bearer <token>`. Negativer Betrag heißt Abbuchung.
Depots stehen **nicht** unter `/konten` – sie kommen über `/positionen`.

Diese Formen sind der Vertrag mit der App und ändern sich nicht stillschweigend.
Wer eine andere Quelle anbinden will, muss nur diese drei Antworten liefern.

## Wie der Abruf läuft

Der Bankdialog passiert **nicht** im HTTP-Aufruf, sondern in einer Schleife im
Hintergrund; die Endpunkte liefern den zuletzt geholten Stand aus `state/`. Ein
FinTS-Dialog dauert Sekunden, und Banken zählen mit – eine App, die bei jedem
Neuzeichnen eine Verbindung aufmacht, fliegt raus.

## Vom Handy aus

Standardmäßig lauscht die Brücke nur auf `127.0.0.1` – also nur auf diesem
Rechner. Für den Zugriff vom Handy:

1. In `config.toml` `adresse = "0.0.0.0:8123"` setzen.
2. `app_ordner` auf den Finanzhelfer-Ordner zeigen lassen. Dann liefert die
   Brücke die App gleich mit aus, und du rufst am Handy
   `http://<IP-des-Rechners>:8123` auf.

Warum nicht die Seite auf GitHub Pages? Die läuft über HTTPS, und ein
HTTPS-Dokument darf keine ungesicherte Verbindung zu einer LAN-Adresse
aufmachen. Über `http://localhost` geht es, über `http://192.168.x.x` nicht.
Wenn die Brücke die App selbst ausliefert, stellt sich die Frage nicht.

Und: dann ist dein Kontostand für jedes Gerät im WLAN erreichbar, das das Token
kennt. Nimm ein langes.

## Sicherheit

- Die Zugangsdaten stehen in `config.toml` auf deiner Platte. Sie gehen an
  **deine Bank** und sonst nirgendwohin.
- `state/` enthält deine Umsätze im Klartext. Beides gehört nicht in eine
  Sicherung, die anderswo landet.
- Der Bitvavo-Schlüssel sollte **nur Leserecht** haben.
- `fints-rs` beherrscht nur lesende Aufträge – Überweisungen sind schlicht
  nicht implementiert. Die Brücke kann kein Geld bewegen.

## Was noch fehlt

- Automatischer Start beim Anmelden (bisher von Hand oder per Aufgabenplanung)
- Verschlüsselte Ablage der `config.toml`
- Erkennung, wann die 90-Tage-Freigabe abläuft, bevor der Abruf scheitert
