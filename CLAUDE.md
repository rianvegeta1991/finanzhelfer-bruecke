# Finanzhelfer-Brücke

Rust-Dienst auf dem eigenen Rechner, der für die [Finanzhelfer-App](../finanzhelfer)
mit den Banken spricht. **Deutsch ist die Quellsprache** (Code, Kommentare, Commits).

Das hier ist ein **eigenes Repo** und läuft **nicht** über GitHub Pages – es ist
kein Web-Projekt, sondern ein Programm, das der Nutzer selbst baut und startet.

## Live

- **Repo:** https://github.com/rianvegeta1991/finanzhelfer-bruecke
- Kein Deploy. `cargo build --release`, fertig.

## Aufbau

| Datei | Inhalt |
|---|---|
| `src/main.rs` | CLI, HTTP-Server, Endpunkte, Hintergrundschleife |
| `src/konfig.rs` | `config.toml` lesen und früh prüfen |
| `src/modell.rs` | die drei Ausgabeformen + `state/`-Ablage |
| `src/quelle_fints.rs` | FinTS über `fints-rs` – nur mit `motor = "rust"`, weist ING und Commerzbank ab |
| `src/quelle_fints_py.rs` + `fints_helfer.py` | FinTS über `python-fints` – **Voreinstellung** |
| `src/quelle_bitvavo.rs` | Bitvavo, offizielle API, HMAC-SHA256 |
| `src/quelle_tr.rs` | Trade Republic über das Fremdwerkzeug `pytr` |

## Die drei Endpunkte sind der Vertrag

`/konten`, `/umsaetze`, `/positionen` – genau so, wie `banking.js` und `kurse.js`
in der App sie abfragen. **Diese Formen nicht stillschweigend ändern**; sie stehen
in beiden READMEs und im Info-Fenster der App. Wer eine Quelle ergänzt, füllt
einen `Bestand` und ist fertig.

Depots tauchen **nicht** unter `/konten` auf: die App kennt nur giro/tagesgeld/
bar/kreditkarte/kredit als Kontoarten und würde ein Depot zu einem Girokonto
machen. Depots kommen über `/positionen?depot=<ref>`.

Ein leerer `depot`-Parameter heißt „alles" – so fragt die App, wenn beim Depot
keine Kennung hinterlegt ist.

## Abruf und Zustand

Der Bankdialog läuft **nie** im HTTP-Aufruf, sondern in der Hintergrundschleife;
die Endpunkte liefern aus `state/<konto-id>.json`. Ein FinTS-Dialog dauert
Sekunden, und Banken begrenzen die Zugriffe.

`state/` hält außerdem die **System-Kennung** je Konto. Sie ist der Grund, warum
nicht jeder Abruf eine TAN verlangt: mit gemerkter Kennung erkennt die Bank das
Gerät wieder (meist ~90 Tage). Sie wird **sofort nach `initiate` gesichert**,
auch wenn der Abruf danach scheitert – sonst holt sich die Bank bei jedem
Versuch eine neue und wird misstrauisch.

## FinTS-Eigenheiten (`fints-rs` 0.2)

- `Flow::initiate(...)` liefert `ChallengeInfo`. `no_tan_required == true` heißt:
  schon angemeldet, direkt abrufen.
- `confirm_and_fetch_opts` meldet **„TAN still pending"**, solange der Nutzer
  nicht bestätigt hat, und stellt dabei seinen Zustand wieder her. **Nur dieser
  eine Fehler darf wiederholt werden**, jeder andere setzt den Flow auf `Done`.
  Deshalb die Prüfung auf den Text – die Bibliothek bietet keinen eigenen Typ.
- `SecurityHolding.acquisition_value` ist der **Gesamt**-Einstandswert, die App
  führt `einstand` als Kurs **je Stück**. Also teilen.
- Vorgemerkte Umsätze werden weggelassen: sie ändern beim Buchen oft Text und
  Betrag und rutschten dann als zweiter Eintrag durch.
- Die Bibliothek kann **nur lesen** – Überweisungen sind nicht implementiert.

## pytr-Eigenheiten

- Aufgerufen wird `export_transactions` (nicht `dl_docs`): das holt die Buchungen
  **ohne** jedes PDF herunterzuladen.
- **`-l de` ist gesetzt**, weil pytr die Kopfzeile übersetzt. Ohne feste Sprache
  heißt sie mal `Datum;Typ;Wert;Notiz;ISIN`, mal `Date;Type;Value;Note;ISIN`.
  Gelesen werden trotzdem beide, aber der Standardfall soll vorhersagbar sein.
- `pytr portfolio -o <datei>` schreibt `Name,ISIN,quantity,price,avgCost,netValue`.
  `avgCost` ist der Einstandskurs je Stück – passt direkt.
- **stdin wird zugenagelt.** Läuft die Anmeldung ab, fragt pytr im Terminal nach
  einer TAN; im Dienst würde das ewig hängen.
- **Die Anmeldung hält nur ein paar Tage.** In `~/.pytr/cookies.*.txt` stehen
  `tr_session`, `tr_refresh` und `tr_claims` als reine **Sitzungs-Cookies** – nur
  `tr_device` gilt ein Jahr. Läuft die Sitzung ab, versucht pytr eine
  Neuanmeldung, landet bei `input("Code: ")` und stirbt mit
  `EOFError: EOF when reading a line`. Am 23.09.2026 angemeldet, am 26.09. schon
  abgelaufen. **Einziger Weg: `.nmelden.ps1 tr` von Hand**, den
  Code tippt der Nutzer selbst. Nicht automatisieren wollen und **nicht in einer
  Schleife probieren** – TR sperrt sonst mit 429 für Stunden.
- Die Brücke liefert bei einem gescheiterten Abruf **weiter ihren letzten guten
  Stand**. Das ist Absicht, macht Ausfälle aber unsichtbar – deshalb nennt
  `/status` je Quelle `stand` und `fehler`, und die App zeigt das seit v1.13 im
  Überblick an. Dieses Feld bitte gefüllt lassen.
- Ein fehlgeschlagener Depotabruf darf die Buchungen nicht mitreißen – deshalb
  steht er in einem eigenen `match` mit bloßer Warnung.
- Standardaufruf ist `python -m pytr`, nicht `pytr`: der Scripts-Ordner von
  Python liegt unter Windows oft nicht im PATH.
- **`playwright install chromium` muss der Nutzer in seiner eigenen Shell
  ausführen.** Aus einer Werkzeug-Sitzung heraus installiert, lagen die Dateien
  zwar am richtigen Ort (geprüft: 211 MB, korrekter Pfad, volle Rechte, auch
  ohne Sandbox sichtbar), und aus denselben Werkzeugen startete der Browser –
  aus Bastians Shell kam trotzdem „Executable doesn't exist". Derselbe Befehl
  aus seiner Shell hat es in Sekunden behoben. Warum, ist ungeklärt; die
  Konsequenz ist: Installationen, die *sein* Terminal benutzen soll, lässt man
  ihn anstoßen oder prüft sie wenigstens von dort.

## FinTS über python-fints (`fints_helfer.py`)

- **Immer `FinTSClientMode.INTERACTIVE`.** `OFFLINE` heißt „überhaupt kein
  Netz" – zum Auswerten gespeicherter Daten – und lässt jeden Bankdialog mit
  `FinTSDialogOfflineError` auflaufen. Ob nachgefragt werden darf, entscheidet
  `tan_erledigen()`, nicht der Modus. Dieser Irrtum hat den Hintergrundabgleich
  lahmgelegt, während die Anmeldung lief.
- `fetch_tan_mechanisms()` **liefert das gewählte Verfahren zurück**;
  `get_tan_mechanisms()` filtert zusätzlich nach den erlaubten Funktionen und
  kann leer bleiben, obwohl eines feststeht. Auf den Rückgabewert verlassen.
- ING lässt den Lesezugriff mit Sicherheitsfunktion **999 ohne TAN** zu und gibt
  nur **90 Tage** Umsätze heraus (sagt es selbst: Rückmeldung 3010).
- stderr des Helfers wird **mitgelesen und zeilenweise durchgereicht**, nicht
  geerbt: mit `inherit` verschwindet die Meldung, sobald der Aufruf umgeleitet
  wird – man sieht dann nur „ist ausgestiegen" ohne Grund.

## Wo es klemmt

- **Produkt-ID der Deutschen Kreditwirtschaft** ist Pflicht für FinTS. Ohne sie
  weisen die meisten Banken ab. Formular unter fints.org/de/hersteller/produktregistrierung, ausgefüllt an
  registrierung@hbci-zka.de; 10–15 Werktage. Eine Registrierung gilt für alle Banken.
  Die alte Adresse hbci-zka.de/register/ ist tot – nur die Mailadresse lebt noch.
- **FNZ Bank (ebase), BLZ 70113000, steht nicht im FinTS-Verzeichnis.** Das Depot
  bleibt Handarbeit. Geprüft mit `fints-institute-db-cli --bankcode 70113000`.
- **Trade Republic hat kein FinTS und keine offizielle API.** Nur `pytr`, mit dem
  AGB-Vorbehalt aus dem README.
- **Mixed Content:** Die App auf GitHub Pages (HTTPS) darf `http://localhost`
  ansprechen, aber **nicht** `http://192.168.x.x`. Fürs Handy deshalb `app_ordner`
  setzen, dann liefert die Brücke die App selbst aus und beide sind dieselbe Herkunft.
- PowerShell 5.1 verflacht verschachtelte Arrays – gilt hier nur für Hilfsskripte,
  aber es kostet sonst Zeit.

## Testen

`cargo test` deckt die Parser ab (pytr-CSV deutsch und englisch, Depotdatei,
Zahlen- und Datumsformate) – die brauchen keine Zugangsdaten.

Die Endpunkte lassen sich **ohne Bank** prüfen: eine `state/<id>.json` von Hand
schreiben, `dienst` starten, mit `curl -H "Authorization: Bearer <token>"`
abfragen. Genau so ist die Schnittstelle gegen die echte App verifiziert worden.

Für den Weg App → Brücke im Browser: `brueckeKonten`, `kontoAbgleichen` und
`depotAbgleichen` sind in der App global und lassen sich per `javascript_tool`
direkt aufrufen.

## „Could not find system_id" heißt fast nie, was es sagt

Weist die Bank den Dialog ab, kommt keine Kundensystem-ID zurück, und
python-fints stirbt mit `ValueError: Could not find system_id`. Der **Grund**
steht in den Antwortsegmenten – bei der Commerzbank durchgehend 9952 „Das
Kundenprodukt wird nicht unterstützt". Wer der Ausnahme glaubt, sucht bei der
Anmeldung statt bei der Produkt-ID.

Deshalb hängt `Mitschrift` (in `fints_helfer.py`) dauerhaft am `fints`-Logger,
sammelt alle `code`/`text`-Paare und gibt sie im Fehlerfall aus;
`bankmeldung_deuten()` macht aus 9952/9010/9931 einen brauchbaren Satz.
Gedruckt wird nur im Fehlerfall, das Protokoll selbst bleibt ohne `--laut` aus.

**Achtung bei deutschen Anführungszeichen in Python-Strings:** „…" mit geradem
Schlusszeichen beendet das Literal. Immer „…" (U+201E/U+201C) benutzen.

## anmelden.ps1 / anmelden.cmd

`anmelden tr` ist kein Befehl, sondern ein Argument für die exe, und die liegt
nicht im PATH. Dafür gibt es `anmelden.ps1` (liest die Konten aus der
config.toml, lässt wählen, ruft die exe mit vollem Pfad auf).

Auf einem frischen Windows ist die Ausführung von .ps1 gesperrt
(`PSSecurityException`), deshalb liegt `anmelden.cmd` daneben: eine .cmd fällt
nicht unter die Sperre und startet die .ps1 mit `-ExecutionPolicy Bypass` –
nur für diesen Aufruf, ohne etwas am System zu ändern. **In der App immer die
.cmd oder die .ps1 nennen, nie den nackten exe-Namen.**
