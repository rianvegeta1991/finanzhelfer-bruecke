//! Trade Republic – der unbequeme Fall.
//!
//! TR hat **keine** offizielle Schnittstelle und kein FinTS. Was bleibt, ist
//! `pytr`, ein von der Community gepflegter Nachbau des internen Protokolls.
//!
//! Zwei Dinge, die du wissen solltest, bevor du das einschaltest:
//!
//!   * TRs Nutzungsbedingungen sehen automatisierten Zugriff nicht vor. Es
//!     sind deine eigenen Daten, aber das Risiko liegt bei dir.
//!   * TR kann sein Protokoll jederzeit ändern. Dann steht der Abruf, bis
//!     `pytr` nachzieht.
//!
//! Deshalb bauen wir das Protokoll hier **nicht selbst** nach, sondern rufen
//! `pytr` auf und übersetzen nur dessen Ausgabe. Der zerbrechliche Teil bleibt
//! bei einem Werkzeug, das viele Leute gemeinsam am Leben halten.
//!
//! Zwei Aufrufe, beide ohne einen einzigen PDF-Download:
//!
//!   pytr export_transactions --outputdir <ziel> --export-format csv
//!   pytr portfolio -o <ziel>/portfolio.csv
//!
//! Angemeldet wird sich **einmalig von Hand** mit `pytr login --store_credentials`
//! (bei Konten mit App-Freigabe zusätzlich `--v2`). Die Sitzung liegt danach in
//! `~/.pytr`; die Brücke bekommt deine PIN nie zu sehen.

use crate::konfig::Konto;
use crate::modell::{Bestand, KontoAus, PositionAus, UmsatzAus};
use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Wohin `pytr` seine Dateien legt.
fn ordner(konto: &Konto) -> PathBuf {
    if konto.pytr_ordner.is_empty() {
        crate::konfig::zustand_ordner().join(&konto.id).join("pytr")
    } else {
        PathBuf::from(&konto.pytr_ordner)
    }
}

/// Einmalige Anmeldung bei Trade Republic.
///
/// Läuft über die Brücke statt direkt im Terminal, weil pytr dann eine
/// Umgebung bekommt, die wir selbst bestimmen – insbesondere den Ort der
/// Playwright-Browser. Der Zwei-Faktor-Code wird vom Nutzer getippt; stdin
/// bleibt deshalb durchgereicht.
pub async fn anmelden(konto: &Konto) -> Result<String> {
    eprintln!("Melde bei Trade Republic an. Der Code kommt als Mitteilung in die TR-App.");
    pytr_interaktiv(konto, &["login".into(), "--store_credentials".into()]).await?;
    Ok(format!("{}: angemeldet", konto.name))
}

pub async fn abgleichen(konto: &Konto, bestand: &mut Bestand) -> Result<String> {
    let ziel = ordner(konto);
    std::fs::create_dir_all(&ziel)
        .with_context(|| format!("{} lässt sich nicht anlegen", ziel.display()))?;

    // ---- Buchungen ----
    // `-l de` ist wichtig: ohne feste Sprache richtet sich pytr nach der
    // Systemsprache, und dann heißt die Kopfzeile mal „Datum;Typ;Wert",
    // mal „Date;Type;Value". Gelesen werden unten trotzdem beide.
    let mut args: Vec<String> = vec![
        "export_transactions".into(),
        "--outputdir".into(),
        ziel.display().to_string(),
        "--export-format".into(),
        "csv".into(),
        "-l".into(),
        "de".into(),
        "-s".into(),
    ];
    if konto.tage > 0 {
        args.push("--last_days".into());
        args.push(konto.tage.to_string());
    }
    pytr_aufrufen(konto, &args).await?;

    let csv = ziel.join("account_transactions.csv");
    if !csv.exists() {
        bail!(
            "pytr hat keine account_transactions.csv in {} hinterlassen. \
             Ist die Anmeldung abgelaufen? Dann einmal `pytr login --store_credentials` im Terminal.",
            ziel.display()
        );
    }
    let umsaetze = csv_lesen(&datei_lesen(&csv)?)?;
    let neu = bestand.umsaetze_zusammenfuehren(umsaetze);

    // ---- Depotbestand ----
    let depot_datei = ziel.join("portfolio.csv");
    let mut positionen = 0;
    let mut gemeldeter_saldo = None;
    match pytr_aufrufen(konto, &["portfolio".into(), "-o".into(), depot_datei.display().to_string()]).await {
        Ok(ausgabe) => {
            gemeldeter_saldo = cash_aus_ausgabe(&ausgabe);
            if depot_datei.exists() {
                let liste = portfolio_lesen(&datei_lesen(&depot_datei)?)?;
                positionen = liste.len();
                if !liste.is_empty() {
                    bestand.positionen = liste;
                }
            } else {
                eprintln!("  ! pytr hat keine portfolio.csv geschrieben – Bestände bleiben, wie sie waren.");
            }
        }
        // Ein fehlgeschlagener Depotabruf darf die Buchungen nicht mitreißen
        Err(e) => eprintln!("  ! Depotbestand von Trade Republic nicht geholt: {e:#}"),
    }

    // Kontostand: am liebsten der, den Trade Republic selbst meldet. Nur wenn
    // der fehlt, die Summe aller Buchungen – die stimmt bloß, wenn `tage` auf 0
    // steht und jede Bewegung im Export auftaucht, und driftet sonst.
    let saldo: f64 = gemeldeter_saldo
        .unwrap_or_else(|| bestand.umsaetze.iter().map(|u| u.betrag).sum());
    bestand.konto = Some(KontoAus {
        kennung: konto.id.clone(),
        name: konto.name.clone(),
        bank: if konto.bank.is_empty() { "Trade Republic".into() } else { konto.bank.clone() },
        iban: konto.iban.clone(),
        art: konto.art.clone(),
        saldo: (saldo * 100.0).round() / 100.0,
        waehrung: "EUR".into(),
    });
    bestand.stand = Some(chrono::Local::now().to_rfc3339());
    bestand.fehler = None;

    Ok(format!("{}: {neu} neue Buchungen, {positionen} Positionen", konto.name))
}

/// `pytr` anstoßen.
///
/// stdin wird zugenagelt: läuft die Anmeldung ab, fragt pytr im Terminal nach
/// einer TAN – im Dienst würde das ewig hängen. So bricht es sauber ab.
/// Wo Playwright seine Browser ablegt.
///
/// Wird das nicht ausdrücklich gesetzt, hängt es an der Umgebung des
/// aufrufenden Prozesses – und die kann je nach Terminal abweichen. Genau
/// daran ist eine Anmeldung schon gescheitert („Chromium is not installed",
/// obwohl es danebenlag). Mit festem Pfad ist es nicht mehr dem Zufall
/// überlassen.
fn browser_ordner() -> Option<String> {
    let lokal = std::env::var("LOCALAPPDATA").ok()?;
    let pfad = std::path::Path::new(&lokal).join("ms-playwright");
    pfad.is_dir().then(|| pfad.display().to_string())
}

/// Interaktiver Aufruf: stdin bleibt offen, damit der Nutzer den Code tippen kann.
async fn pytr_interaktiv(konto: &Konto, args: &[String]) -> Result<()> {
    let (programm, vorlauf) = konto.pytr_befehl();
    let mut befehl = tokio::process::Command::new(&programm);
    befehl
        .args(&vorlauf)
        .args(args)
        .args(konto.pytr_argumente.iter())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(ordner) = browser_ordner() {
        befehl.env("PLAYWRIGHT_BROWSERS_PATH", ordner);
    }
    let status = befehl
        .status()
        .await
        .map_err(|e| anyhow!("`{programm}` lässt sich nicht starten ({e})."))?;
    if !status.success() {
        bail!("pytr ist ausgestiegen ({status}). Die Meldung steht darüber.");
    }
    Ok(())
}

async fn pytr_aufrufen(konto: &Konto, args: &[String]) -> Result<String> {
    let (programm, vorlauf) = konto.pytr_befehl();
    let mut vorbereitet = tokio::process::Command::new(&programm);
    if let Some(ordner) = browser_ordner() {
        vorbereitet.env("PLAYWRIGHT_BROWSERS_PATH", ordner);
    }
    let ausgabe = vorbereitet
        .args(&vorlauf)
        .args(args)
        .args(konto.pytr_argumente.iter())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| {
            anyhow!(
                "`{programm}` lässt sich nicht starten ({e}).\n\
                 Installiert wird pytr mit `python -m pip install pytr`. Findet der Dienst es \
                 trotzdem nicht, kennt seine Umgebung den PATH von Python nicht – dann in der \
                 config.toml `pytr_programm` auf den vollen Pfad zu pytr.exe setzen \
                 (in EINFACHEN Anführungszeichen, sonst verschluckt TOML die Backslashes)."
            )
        })?;

    if !ausgabe.status.success() {
        // pytr schreibt seine Meldungen auf stderr; die letzten Zeilen sagen,
        // was los ist (meist: Anmeldung abgelaufen)
        let fehler = String::from_utf8_lossy(&ausgabe.stderr);
        let letzte: Vec<&str> = fehler.lines().filter(|z| !z.trim().is_empty()).rev().take(4).collect();
        bail!(
            "pytr ist ausgestiegen ({}): {}",
            ausgabe.status,
            letzte.into_iter().rev().collect::<Vec<_>>().join(" / ")
        );
    }
    Ok(String::from_utf8_lossy(&ausgabe.stdout).into_owned())
}

/// Liest den echten Kontostand aus der Ausgabe von `pytr portfolio`.
///
/// Die Zeilen `Depot`, `Cash` und `Total` druckt pytr auch dann, wenn die
/// Bestände über `-o` in eine Datei gehen. Das ist der Saldo, den Trade
/// Republic selbst meldet – deutlich verlässlicher, als ihn aus der Summe
/// aller Buchungen zu erschließen.
fn cash_aus_ausgabe(ausgabe: &str) -> Option<f64> {
    for zeile in ausgabe.lines() {
        let z = zeile.trim();
        if let Some(rest) = z.strip_prefix("Cash ") {
            // "Cash EUR       11500.00"
            let mut teile = rest.split_whitespace();
            let _waehrung = teile.next()?;
            let betrag = teile.next()?;
            if let Ok(w) = betrag.replace(',', "").parse::<f64>() {
                return Some(w);
            }
        }
    }
    None
}

/// pytr schreibt UTF-8; falls doch nicht, nicht daran scheitern.
fn datei_lesen(pfad: &Path) -> Result<String> {
    let roh = std::fs::read(pfad).with_context(|| format!("{} lässt sich nicht lesen", pfad.display()))?;
    Ok(String::from_utf8_lossy(&roh).into_owned())
}

// ───────────────────────── CSV lesen ─────────────────────────

/// Kopfzeile in Kleinbuchstaben plus Trennzeichen.
fn kopf_lesen(roh: &str) -> Option<(char, Vec<String>, Vec<&str>)> {
    let mut zeilen = roh.lines().filter(|z| !z.trim().is_empty());
    let kopfzeile = zeilen.next()?;
    let trenner = if kopfzeile.matches(';').count() > kopfzeile.matches(',').count() { ';' } else { ',' };
    let kopf = kopfzeile
        .split(trenner)
        .map(|s| s.trim().trim_matches('"').to_lowercase())
        .collect();
    Some((trenner, kopf, zeilen.collect()))
}

/// Spalte nach Bedeutung suchen: erst genau, dann enthalten. Die Spaltennamen
/// haben sich zwischen pytr-Fassungen schon geändert.
fn spalte(kopf: &[String], muster: &[&str]) -> Option<usize> {
    for m in muster {
        if let Some(i) = kopf.iter().position(|k| k == m) {
            return Some(i);
        }
    }
    for m in muster {
        if let Some(i) = kopf.iter().position(|k| k.contains(m)) {
            return Some(i);
        }
    }
    None
}

fn feld(teile: &[&str], i: Option<usize>) -> String {
    i.and_then(|i| teile.get(i))
        .map(|s| s.trim().trim_matches('"').to_string())
        .unwrap_or_default()
}

fn csv_lesen(roh: &str) -> Result<Vec<UmsatzAus>> {
    let (trenner, kopf, zeilen) = kopf_lesen(roh).ok_or_else(|| anyhow!("Die CSV von pytr ist leer"))?;

    // Die Kopfzeile ist übersetzt – deutsche Namen zuerst, englische als Rückfall
    let i_datum = spalte(&kopf, &["datum", "date", "timestamp"])
        .ok_or_else(|| anyhow!("Keine Datumsspalte in der pytr-CSV (Kopf: {})", kopf.join(", ")))?;
    let i_betrag = spalte(&kopf, &["wert", "value", "betrag", "amount"])
        .ok_or_else(|| anyhow!("Keine Betragsspalte in der pytr-CSV (Kopf: {})", kopf.join(", ")))?;
    let i_typ = spalte(&kopf, &["typ", "type", "art"]);
    let i_notiz = spalte(&kopf, &["notiz", "note", "beschreibung", "description"]);
    let i_isin = spalte(&kopf, &["isin"]);

    let mut raus = Vec::new();
    for zeile in zeilen {
        let teile: Vec<&str> = zeile.split(trenner).collect();
        if teile.len() <= i_datum.max(i_betrag) {
            continue;
        }
        let Some(datum) = datum_lesen(teile[i_datum].trim().trim_matches('"')) else { continue };
        let Some(betrag) = betrag_lesen(teile[i_betrag].trim().trim_matches('"')) else { continue };

        let typ = feld(&teile, i_typ);
        let notiz = feld(&teile, i_notiz);
        let isin = feld(&teile, i_isin);

        // In die Gegenseite gehört der Wertpapiername aus der Notiz: danach
        // gruppiert die App ihre Vertragserkennung, und so wird aus einem
        // Sparplan eine erkennbare, wiederkehrende Zahlung.
        let gegen = if !notiz.is_empty() {
            notiz.clone()
        } else if !typ.is_empty() {
            typ.clone()
        } else {
            "Trade Republic".into()
        };

        raus.push(UmsatzAus {
            datum,
            betrag,
            gegen,
            zweck: [typ, isin]
                .iter()
                .filter(|s| !s.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" · "),
            waehrung: "EUR".into(),
        });
    }
    Ok(raus)
}

/// `pytr portfolio -o` schreibt Name, ISIN, quantity, price, avgCost, netValue.
fn portfolio_lesen(roh: &str) -> Result<Vec<PositionAus>> {
    let (trenner, kopf, zeilen) = kopf_lesen(roh).ok_or_else(|| anyhow!("Die Depotdatei von pytr ist leer"))?;

    let i_name = spalte(&kopf, &["name", "bezeichnung"]);
    let i_isin = spalte(&kopf, &["isin"]);
    let i_menge = spalte(&kopf, &["quantity", "stueck", "anzahl"])
        .ok_or_else(|| anyhow!("Keine Stückzahl in der Depotdatei (Kopf: {})", kopf.join(", ")))?;
    let i_kurs = spalte(&kopf, &["price", "kurs"]);
    let i_einstand = spalte(&kopf, &["avgcost", "einstand", "avg"]);

    let mut raus = Vec::new();
    for zeile in zeilen {
        let teile: Vec<&str> = zeile.split(trenner).collect();
        if teile.len() <= i_menge {
            continue;
        }
        let Some(stueck) = betrag_lesen(&feld(&teile, Some(i_menge))) else { continue };
        if stueck <= 0.0 {
            continue;
        }
        let isin = feld(&teile, i_isin);
        let name = {
            let n = feld(&teile, i_name);
            if n.is_empty() { isin.clone() } else { n }
        };
        let einstand = betrag_lesen(&feld(&teile, i_einstand)).unwrap_or(0.0);
        let mut kurs = betrag_lesen(&feld(&teile, i_kurs)).unwrap_or(0.0);

        // Anleihen notiert Trade Republic teils in **Prozent vom Nennwert**
        // (104,75), den Einstand daneben aber als Faktor (1,0877). pytr rechnet
        // stur Stück × Kurs und kommt so auf das Hundertfache: aus 468 € wurden
        // 46.844 €. Erkennbar ist es nur am Verhältnis der beiden Zahlen –
        // ein echter Gewinn in dieser Größenordnung kommt nicht vor.
        if einstand > 0.0 {
            let verhaeltnis = kurs / einstand;
            if (50.0..=200.0).contains(&verhaeltnis) {
                eprintln!(
                    "  i {name}: Kurs {kurs} sieht nach Prozent vom Nennwert aus (Einstand {einstand}) – durch 100 geteilt."
                );
                kurs /= 100.0;
            }
        }

        raus.push(PositionAus {
            art: art_raten(&name, &isin),
            name,
            isin,
            wkn: String::new(),
            symbol: String::new(),
            stueck,
            einstand,
            kurs,
            waehrung: "EUR".into(),
        });
    }
    Ok(raus)
}

/// Grobe Einordnung aus dem Namen. Falsch geraten ist folgenlos – die Art
/// steuert nur das Zeichen in der Liste, nicht die Rechnung.
fn art_raten(name: &str, isin: &str) -> String {
    let n = name.to_uppercase();
    if n.contains("ETF") || n.contains("UCITS") {
        "etf".into()
    } else if n.contains("FONDS") || n.contains("FUND") {
        "fonds".into()
    } else if n.contains("BOND") || n.contains("ANLEIHE") {
        "anleihe".into()
    } else if isin.is_empty() {
        "krypto".into() // TR führt Krypto ohne ISIN
    } else {
        "aktie".into()
    }
}

/// pytr schreibt ISO-Zeitstempel; die App will nur den Tag.
fn datum_lesen(s: &str) -> Option<String> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
        return Some(s[..10].to_string());
    }
    // Notnagel: TT.MM.JJJJ
    let t: Vec<&str> = s.split('.').collect();
    if t.len() == 3 && t[2].len() >= 4 {
        return Some(format!("{}-{:0>2}-{:0>2}", &t[2][..4], t[1], t[0]));
    }
    None
}

/// Deutsch wie englisch geschrieben. pytr liefert je nach `--decimal-localization`
/// mal das eine, mal das andere.
fn betrag_lesen(s: &str) -> Option<f64> {
    let mut t: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == ',' || *c == '.' || *c == '-')
        .collect();
    if t.is_empty() || t == "-" {
        return None;
    }
    match (t.rfind(','), t.rfind('.')) {
        // Das hintere Zeichen ist das Dezimaltrennzeichen
        (Some(k), Some(p)) if k > p => t = t.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => t = t.replace(',', ""),
        (Some(_), None) => t = t.replace(',', "."),
        _ => {}
    }
    t.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kopfzeile wie pytr sie mit `-l de` schreibt.
    #[test]
    fn liest_pytr_buchungen_deutsch() {
        let roh = "Datum,Typ,Wert,Notiz,ISIN\n\
                   2026-09-01T08:00:00,Kauf,-300.00,Vanguard FTSE All-World UCITS ETF,IE00BK5BQT80\n\
                   2026-08-15T10:12:00,Zinsen,12.44,Guthabenzinsen,\n";
        let erg = csv_lesen(roh).unwrap();
        assert_eq!(erg.len(), 2);
        assert_eq!(erg[0].datum, "2026-09-01");
        assert_eq!(erg[0].betrag, -300.0);
        // Der Wertpapiername gehört in die Gegenseite – daran hängt die
        // Vertragserkennung der App
        assert_eq!(erg[0].gegen, "Vanguard FTSE All-World UCITS ETF");
        assert_eq!(erg[0].zweck, "Kauf · IE00BK5BQT80");
        assert_eq!(erg[1].gegen, "Guthabenzinsen");
        assert_eq!(erg[1].betrag, 12.44);
    }

    /// Und wie mit `-l en`, falls jemand die Sprache überschreibt.
    #[test]
    fn liest_pytr_buchungen_englisch() {
        let roh = "Date;Type;Value;Note;ISIN\n\
                   2026-09-01T08:00:00;Buy;-300.00;Vanguard FTSE All-World;IE00BK5BQT80\n";
        let erg = csv_lesen(roh).unwrap();
        assert_eq!(erg.len(), 1);
        assert_eq!(erg[0].gegen, "Vanguard FTSE All-World");
        assert_eq!(erg[0].betrag, -300.0);
    }

    #[test]
    fn liest_pytr_depot() {
        let roh = "Name,ISIN,quantity,price,avgCost,netValue\n\
                   Vanguard FTSE All-World UCITS ETF,IE00BK5BQT80,42.5,118.62,98.40,5041.35\n\
                   Apple Inc.,US0378331005,15,224.15,162.00,3362.25\n";
        let erg = portfolio_lesen(roh).unwrap();
        assert_eq!(erg.len(), 2);
        assert_eq!(erg[0].stueck, 42.5);
        assert_eq!(erg[0].einstand, 98.40);
        assert_eq!(erg[0].art, "etf");
        assert_eq!(erg[1].art, "aktie");
        assert_eq!(erg[1].kurs, 224.15);
    }

    /// Echte Zeile aus einem TR-Depot: Kurs in Prozent, Einstand als Faktor.
    /// Unkorrigiert kämen 46.844 € statt 468 € heraus.
    #[test]
    fn rechnet_anleihe_in_prozent_um() {
        let roh = "Name;ISIN;quantity;price;avgCost;netValue\n\
                   Sept. 2033;XS2680932907;447.2;104.75;1.0877;46844.2\n";
        let erg = portfolio_lesen(roh).unwrap();
        assert_eq!(erg.len(), 1);
        assert!((erg[0].kurs - 1.0475).abs() < 1e-9, "Kurs war {}", erg[0].kurs);
        let wert = erg[0].stueck * erg[0].kurs;
        assert!((wert - 468.44).abs() < 0.01, "Wert war {wert}");
    }

    /// Eine Anleihe, bei der beide Zahlen schon dieselbe Einheit haben,
    /// darf nicht angefasst werden.
    #[test]
    fn laesst_stimmige_anleihe_in_ruhe() {
        let roh = "Name;ISIN;quantity;price;avgCost;netValue\n\
                   Mai 2037;XS2829810923;2563.02;0.9327;0.9698;2390.53\n";
        let erg = portfolio_lesen(roh).unwrap();
        assert_eq!(erg[0].kurs, 0.9327);
    }

    #[test]
    fn liest_den_saldo_aus_der_ausgabe() {
        let ausgabe = "Depot      29000.00 ->   30403.57    1403.57     4.8%\n\
                       Cash EUR                              11500.25\n\
                       Total      40500.00 ->   41903.82\n";
        assert_eq!(cash_aus_ausgabe(ausgabe), Some(11500.25));
        assert_eq!(cash_aus_ausgabe("nichts dergleichen"), None);
    }

    #[test]
    fn liest_beide_zahlenformate() {
        assert_eq!(betrag_lesen("-1,234.56"), Some(-1234.56));
        assert_eq!(betrag_lesen("1.234,56"), Some(1234.56));
        assert_eq!(betrag_lesen("42"), Some(42.0));
        assert_eq!(betrag_lesen(""), None);
    }

    #[test]
    fn liest_datumsformate() {
        assert_eq!(datum_lesen("2026-09-01T08:00:00"), Some("2026-09-01".into()));
        assert_eq!(datum_lesen("01.09.2026"), Some("2026-09-01".into()));
        assert_eq!(datum_lesen("Unsinn"), None);
    }
}
