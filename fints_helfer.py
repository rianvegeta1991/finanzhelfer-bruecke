"""FinTS-Abruf über python-fints – aufgerufen von der Brücke.

Warum ein Python-Helfer neben der Rust-Brücke?
    `fints-rs` 0.2 baut für ING und Commerzbank Nachrichten, die die Banken
    zurückweisen (9010 „Ungültiger Signaturaufbau"). `python-fints` ist seit
    Jahren mit denselben Banken im Einsatz. Dieselbe Bauart wie bei Trade
    Republic: der zerbrechliche Teil bleibt bei einer gepflegten Bibliothek,
    die Brücke übersetzt nur deren Ausgabe.

Aufruf:
    python fints_helfer.py anmelden    einmalig, mit TAN-Freigabe
    python fints_helfer.py abrufen     still, nutzt die gemerkte Anmeldung

Alles über Umgebungsvariablen, damit die PIN nicht in der Befehlszeile und
damit auch nicht in der Prozessliste steht:

    FH_BLZ FH_URL FH_USER FH_PIN FH_PRODUKT FH_IBAN FH_ART FH_TAGE FH_STATE

Auf **stdout** kommt genau ein JSON-Objekt in den Formen der Brücke:
    {"konto": {...}, "umsaetze": [...], "positionen": [...]}
Alles Menschenlesbare geht auf stderr.
"""

import datetime
import json
import logging
import os
import sys
from decimal import Decimal

from fints.client import FinTS3PinTanClient, FinTSClientMode, NeedTANResponse

# Mit FH_LAUT=1 (die Brücke setzt es bei `--laut`) protokolliert python-fints
# den ganzen Dialog. Ohne das sieht man bei einer Abweisung nur den nackten
# Ausnahmetext – die Bank schreibt den Grund aber in ihre Rückmeldungen.
if os.environ.get("FH_LAUT"):
    logging.basicConfig(level=logging.DEBUG, stream=sys.stderr,
                        format="  [%(name)s] %(message)s")


def sag(*teile):
    """Meldung an den Menschen – niemals nach stdout, das gehört dem JSON."""
    print(*teile, file=sys.stderr, flush=True)


def umgebung(name, pflicht=True, standard=""):
    wert = os.environ.get(name, standard)
    if pflicht and not wert:
        sag(f"Fehler: {name} ist nicht gesetzt.")
        sys.exit(2)
    return wert


def zahl(wert):
    """Decimal/Amount/None → float, für die JSON-Ausgabe."""
    if wert is None:
        return 0.0
    if isinstance(wert, Decimal):
        return float(wert)
    if hasattr(wert, "amount"):
        return float(wert.amount)
    try:
        return float(wert)
    except (TypeError, ValueError):
        return 0.0


def datum(wert):
    if isinstance(wert, (datetime.date, datetime.datetime)):
        return wert.strftime("%Y-%m-%d")
    return str(wert or "")[:10]


# ───────────────────────── Anmeldung ─────────────────────────

def zustand_laden(pfad):
    try:
        with open(pfad, "rb") as f:
            return f.read()
    except FileNotFoundError:
        return None


def zustand_sichern(pfad, klient):
    os.makedirs(os.path.dirname(pfad) or ".", exist_ok=True)
    with open(pfad, "wb") as f:
        f.write(klient.deconstruct(including_private=True))
    # Die Datei enthält die Sitzung – nicht für andere Konten lesbar machen
    try:
        os.chmod(pfad, 0o600)
    except OSError:
        pass


def tan_erledigen(klient, antwort, interaktiv):
    """Eine TAN-Aufforderung abarbeiten.

    Bei der Push-Freigabe (`decoupled`) wird ohne TAN wiederholt nachgefragt,
    bis der Nutzer in der Banking-App bestätigt hat. Sonst muss er sie tippen.
    """
    if not interaktiv:
        raise SystemExit(
            "Die Bank verlangt eine TAN. Einmalig im Terminal ausführen:\n"
            "  finanzhelfer-bruecke anmelden <konto-id>"
        )

    sag("")
    sag("  ┌─ Freigabe nötig ─────────────────────────")
    for zeile in str(antwort.challenge or "").splitlines():
        sag(f"  │ {zeile}")
    sag("  └──────────────────────────────────────────")

    if getattr(antwort, "decoupled", False):
        sag("  Bestätige das jetzt in deiner Banking-App …")
        import time
        for versuch in range(60):          # 60 × 3 s = drei Minuten Geduld
            time.sleep(3)
            ergebnis = klient.send_tan(antwort, "")
            if not isinstance(ergebnis, NeedTANResponse):
                sag("  ✓ bestätigt")
                return ergebnis
            sag(f"  … warte ({(versuch + 1) * 3}s)")
        raise SystemExit("Keine Bestätigung innerhalb von drei Minuten.")

    tan = input("  TAN eingeben: ").strip()
    return klient.send_tan(antwort, tan)


def klient_bauen(interaktiv):
    zustand = umgebung("FH_STATE")
    daten = zustand_laden(zustand)

    klient = FinTS3PinTanClient(
        umgebung("FH_BLZ"),
        umgebung("FH_USER"),
        umgebung("FH_PIN"),
        umgebung("FH_URL"),
        product_id=umgebung("FH_PRODUKT"),
        product_version="1.0",
        from_data=daten,
        # Immer INTERACTIVE. OFFLINE heißt bei python-fints „überhaupt kein
        # Netz" – gedacht zum Auswerten gespeicherter Daten – und lässt jeden
        # Bankdialog mit FinTSDialogOfflineError auflaufen. Ob nachgefragt
        # werden darf, entscheidet unten tan_erledigen(), nicht der Modus.
        mode=FinTSClientMode.INTERACTIVE,
    )

    # Beim ersten Mal das TAN-Verfahren festlegen. Später steckt es im Zustand.
    if daten is None:
        # Liefert das bereits gewählte Verfahren zurück – das ist der Normalfall.
        # get_tan_mechanisms() filtert zusätzlich nach den erlaubten Funktionen
        # und kann dabei leer bleiben, obwohl ein Verfahren feststeht; darauf
        # darf man sich also nicht allein verlassen.
        gewaehlt = klient.fetch_tan_mechanisms()
        verfahren = klient.get_tan_mechanisms()

        if not gewaehlt:
            if not verfahren:
                raise SystemExit(
                    "Die Bank nennt kein nutzbares TAN-Verfahren. "
                    "Ist der FinTS-Zugang im Online-Banking freigeschaltet?"
                )
            gewaehlt = list(verfahren.keys())[0]
            klient.set_tan_mechanism(gewaehlt)

        name = verfahren[gewaehlt].name if gewaehlt in verfahren else "unbenannt"
        sag(f"  TAN-Verfahren: {name} ({gewaehlt})")

        # Manche Banken verlangen zusätzlich die Wahl des TAN-Mediums
        try:
            medien = klient.get_tan_media()
            liste = medien[1] if isinstance(medien, tuple) else medien
            if liste and len(liste) == 1:
                klient.set_tan_medium(liste[0])
                sag(f"  TAN-Medium: {liste[0].tan_medium_name}")
            elif liste and len(liste) > 1:
                sag("  Mehrere TAN-Medien vorhanden:")
                for i, m in enumerate(liste, 1):
                    sag(f"    {i}) {m.tan_medium_name}")
                wahl = input("  Welches? [1] ").strip() or "1"
                klient.set_tan_medium(liste[int(wahl) - 1])
        except Exception as e:                     # noqa: BLE001
            # Nicht jede Bank kennt HKTAB – das ist kein Grund aufzugeben
            sag(f"  (kein TAN-Medium abgefragt: {type(e).__name__})")

    return klient, zustand


# ───────────────────────── Abruf ─────────────────────────

def konto_finden(klient, iban):
    konten = klient.get_sepa_accounts()
    if not konten:
        raise SystemExit("Die Bank meldet kein Konto zu diesem Zugang.")
    gesucht = (iban or "").replace(" ", "").upper()
    for k in konten:
        if gesucht and (k.iban or "").upper() == gesucht:
            return k
    if gesucht:
        sag(f"  ! IBAN {gesucht} war nicht dabei – nehme das erste von {len(konten)}.")
        sag("    Gefunden: " + ", ".join(k.iban or k.accountnumber for k in konten))
    return konten[0]


def umsaetze_holen(klient, konto, tage):
    von = datetime.date.today() - datetime.timedelta(days=max(1, tage))
    roh = klient.get_transactions(konto, von, datetime.date.today())
    raus = []
    for t in roh:
        d = t.data
        betrag = d.get("amount")
        raus.append({
            "datum": datum(d.get("date")),
            "betrag": zahl(betrag),
            "gegen": (d.get("applicant_name") or "").strip(),
            "zweck": (d.get("purpose") or d.get("posting_text") or "").strip(),
            "waehrung": getattr(betrag, "currency", "EUR") or "EUR",
        })
    return raus


def bestaende_holen(klient, konto):
    raus = []
    for h in klient.get_holdings(konto) or []:
        stueck = zahl(h.pieces)
        # Die Bank liefert den Gesamt-Einstandswert, die App will den Kurs je
        # Stück – dieselbe Umrechnung wie im Rust-Teil.
        einstand = zahl(h.acquisitionprice)
        raus.append({
            "name": (h.name or h.ISIN or "").strip(),
            "isin": h.ISIN or "",
            "wkn": "",
            "symbol": "",
            "art": art_raten(h.name or ""),
            "stueck": stueck,
            "einstand": einstand,
            "kurs": zahl(h.market_value),
            "waehrung": h.value_symbol or "EUR",
        })
    return raus


def art_raten(name):
    n = (name or "").upper()
    if "ETF" in n or "UCITS" in n:
        return "etf"
    if "FONDS" in n or "FUND" in n:
        return "fonds"
    if "ANLEIHE" in n or "BOND" in n:
        return "anleihe"
    return "aktie"


# ───────────────────────── Hauptlauf ─────────────────────────

def main():
    befehl = sys.argv[1] if len(sys.argv) > 1 else "abrufen"
    interaktiv = befehl == "anmelden"

    klient, zustand_pfad = klient_bauen(interaktiv)
    ergebnis = {"konto": None, "umsaetze": [], "positionen": []}

    with klient:
        # Eine TAN direkt bei der Anmeldung (kommt bei ING regelmäßig vor)
        if isinstance(klient.init_tan_response, NeedTANResponse):
            tan_erledigen(klient, klient.init_tan_response, interaktiv)

        konto = konto_finden(klient, os.environ.get("FH_IBAN", ""))
        art = os.environ.get("FH_ART", "giro")

        if art == "depot":
            ergebnis["positionen"] = bestaende_holen(klient, konto)
            sag(f"  {len(ergebnis['positionen'])} Positionen")
        else:
            saldo = klient.get_balance(konto)
            ergebnis["umsaetze"] = umsaetze_holen(klient, konto, int(os.environ.get("FH_TAGE") or 90))
            ergebnis["konto"] = {
                "ref": os.environ.get("FH_REF", ""),
                "name": os.environ.get("FH_NAME", "Konto"),
                "bank": os.environ.get("FH_BANK", ""),
                "iban": konto.iban or "",
                "art": art,
                "saldo": zahl(getattr(saldo, "amount", saldo)),
                "waehrung": getattr(getattr(saldo, "amount", None), "currency", "EUR") or "EUR",
            }
            sag(f"  {len(ergebnis['umsaetze'])} Buchungen, Saldo {ergebnis['konto']['saldo']:.2f}")

    # Erst nach erfolgreichem Lauf sichern – eine halbe Anmeldung nützt nichts
    zustand_sichern(zustand_pfad, klient)
    json.dump(ergebnis, sys.stdout, ensure_ascii=False)
    sys.stdout.flush()


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception as fehler:                       # noqa: BLE001
        sag(f"Fehler: {type(fehler).__name__}: {fehler}")
        sys.exit(1)
