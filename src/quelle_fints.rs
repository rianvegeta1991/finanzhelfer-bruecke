//! FinTS/HBCI – der direkte Draht zur Bank, ohne Dritte dazwischen.
//!
//! Ablauf in zwei Stufen, und das aus gutem Grund:
//!
//!   1. `bruecke anmelden <konto>` – einmalig, im Terminal. Die Bank schickt
//!      eine TAN-Aufforderung (bei ING und Commerzbank als Push in die App),
//!      du bestätigst sie, und wir merken uns die **System-Kennung**.
//!   2. Danach läuft der Abgleich still: mit gemerkter System-Kennung erkennt
//!      die Bank das Gerät wieder und verlangt meist ~90 Tage lang keine
//!      weitere TAN. Läuft das ab, meldet der Dienst es, und Schritt 1
//!      wiederholt sich.
//!
//! Warum nicht alles im laufenden Dienst? Weil eine TAN-Bestätigung einen
//! Menschen braucht. Ein HTTP-GET, der minutenlang auf ein Handy wartet, ist
//! keine brauchbare Schnittstelle.

use crate::konfig::Konto;
use crate::modell::{Bestand, KontoAus, PositionAus, UmsatzAus, dez};
use anyhow::{Result, anyhow, bail};
use fints::{
    Bic, FetchOpts, Flow, Iban, Pin, ProductId, SyncResult, SystemId, TransactionStatus, UserId,
};
use std::time::Duration;

/// Wie lange auf die TAN-Bestätigung gewartet wird, bevor wir aufgeben.
const TAN_GEDULD: Duration = Duration::from_secs(180);
const TAN_TAKT: Duration = Duration::from_secs(3);

/// Holt Saldo, Umsätze und – bei einem Depot – die Bestände.
///
/// `interaktiv` entscheidet, was bei einer TAN-Abfrage passiert: im Terminal
/// warten wir auf die Bestätigung, im Dienst brechen wir mit einem Hinweis ab.
pub async fn abgleichen(
    konto: &Konto,
    produkt_id: &str,
    bestand: &mut Bestand,
    interaktiv: bool,
) -> Result<String> {
    let benutzer = UserId::new(konto.benutzer.clone());
    let pin = Pin::new(konto.pin());
    let produkt = ProductId::new(produkt_id.to_string());
    // Die gemerkte Kennung muss die Initiierung überleben, deshalb erst binden
    let gemerkt = bestand.system_id.clone().map(SystemId::new);
    let iban = Iban::new(konto.iban.clone());
    let bic = (!konto.bic.is_empty()).then(|| Bic::new(konto.bic.clone()));

    let (mut flow, aufforderung) = Flow::initiate(
        &konto.blz,
        &benutzer,
        &pin,
        &produkt,
        gemerkt.as_ref(),
        Some(&iban),
        bic.as_ref(),
    )
    .await
    .map_err(|e| anyhow!("Anmeldung bei {} fehlgeschlagen: {e}", konto.bank))?;

    // Die System-Kennung sofort sichern – auch wenn der Abruf danach scheitert.
    // Sonst holt sich die Bank bei jedem Versuch eine neue und wird misstrauisch.
    bestand.system_id = Some(flow.system_id().as_str().to_string());

    if !aufforderung.no_tan_required {
        if !interaktiv {
            bail!(
                "{} verlangt eine TAN. Führe im Terminal `finanzhelfer-bruecke anmelden {}` aus \
                 und bestätige die Freigabe in der Banking-App.",
                konto.bank,
                konto.id
            );
        }
        println!();
        println!("  ┌─ {} braucht eine Freigabe ─────────────", konto.bank);
        println!("  │ {}", aufforderung.challenge);
        if aufforderung.decoupled {
            println!("  │");
            println!("  │ Bestätige das jetzt in deiner Banking-App.");
        }
        println!("  └────────────────────────────────────────────");
        println!();
    }

    let opts = if konto.ist_depot() {
        // Beim Depot nur die Bestände: der Dialog bleibt kurz, und Umsätze
        // hat ein Depotkonto ohnehin keine, die die App gebrauchen könnte.
        FetchOpts { balance: false, transactions: false, holdings: true, days: 0 }
    } else {
        FetchOpts::no_holdings(konto.tage)
    };

    let ergebnis = holen_mit_geduld(&mut flow, konto, &opts, interaktiv).await?;

    if let Some(id) = &ergebnis.system_id {
        bestand.system_id = Some(id.as_str().to_string());
    }
    Ok(uebernehmen(konto, bestand, ergebnis))
}

/// Ruft ab und wartet dabei auf die TAN-Bestätigung.
///
/// `confirm_and_fetch_opts` meldet „TAN still pending", solange der Nutzer
/// nicht bestätigt hat, und stellt seinen Zustand dabei wieder her – genau
/// dieser eine Fehler darf also wiederholt werden, jeder andere nicht.
async fn holen_mit_geduld(
    flow: &mut Flow,
    konto: &Konto,
    opts: &FetchOpts,
    interaktiv: bool,
) -> Result<SyncResult> {
    let beginn = std::time::Instant::now();
    let mut punkte = 0;
    loop {
        match flow.confirm_and_fetch_opts(&konto.iban, &konto.bic, opts).await {
            Ok(erg) => {
                if punkte > 0 {
                    println!();
                }
                return Ok(erg);
            }
            Err(e) => {
                let text = e.to_string();
                let wartet = text.contains("TAN still pending");
                if !wartet {
                    return Err(anyhow!("Abruf bei {} fehlgeschlagen: {text}", konto.bank));
                }
                if !interaktiv {
                    bail!(
                        "{} wartet auf die TAN-Bestätigung. Führe `finanzhelfer-bruecke anmelden {}` aus.",
                        konto.bank,
                        konto.id
                    );
                }
                if beginn.elapsed() > TAN_GEDULD {
                    println!();
                    bail!(
                        "Nach {} Sekunden kam keine Bestätigung. Versuch es noch einmal.",
                        TAN_GEDULD.as_secs()
                    );
                }
                if punkte == 0 {
                    print!("  Warte auf die Bestätigung ");
                }
                print!(".");
                use std::io::Write;
                std::io::stdout().flush().ok();
                punkte += 1;
                tokio::time::sleep(TAN_TAKT).await;
            }
        }
    }
}

/// Das Bankergebnis in die Formen der App übersetzen.
fn uebernehmen(konto: &Konto, bestand: &mut Bestand, erg: SyncResult) -> String {
    let waehrung = erg
        .balance
        .as_ref()
        .map(|b| b.currency.as_str().to_string())
        .unwrap_or_else(|| "EUR".into());

    if !konto.ist_depot() {
        bestand.konto = Some(KontoAus {
            kennung: konto.id.clone(),
            name: konto.name.clone(),
            bank: konto.bank.clone(),
            iban: konto.iban.clone(),
            art: konto.art.clone(),
            saldo: erg.balance.as_ref().map(|b| dez(b.amount)).unwrap_or(0.0),
            waehrung,
        });
    }

    // Nur gebuchte Umsätze. Vorgemerkte ändern beim Buchen oft Text und
    // Betrag – sie würden als zweiter Eintrag durchrutschen.
    let umsaetze: Vec<UmsatzAus> = erg
        .transactions
        .iter()
        .filter(|t| t.status == TransactionStatus::Booked)
        .map(|t| UmsatzAus {
            datum: t.date.to_string(),
            betrag: dez(t.amount),
            gegen: t.applicant_name.clone().unwrap_or_default(),
            zweck: t
                .purpose
                .clone()
                .or_else(|| t.posting_text.clone())
                .unwrap_or_default(),
            waehrung: t.currency.as_str().to_string(),
        })
        .collect();
    let neu = bestand.umsaetze_zusammenfuehren(umsaetze);

    if !erg.holdings.is_empty() {
        bestand.positionen = erg.holdings.iter().map(position_aus).collect();
    }

    bestand.stand = Some(chrono::Local::now().to_rfc3339());
    bestand.fehler = None;

    format!(
        "{}: {} neue Umsätze, {} Positionen{}",
        konto.name,
        neu,
        bestand.positionen.len(),
        bestand
            .konto
            .as_ref()
            .map(|k| format!(", Saldo {:.2} {}", k.saldo, k.waehrung))
            .unwrap_or_default()
    )
}

fn position_aus(h: &fints::SecurityHolding) -> PositionAus {
    let stueck = dez(h.quantity);
    // Die App führt `einstand` als Kurs je Stück, die Bank liefert den
    // Gesamt-Einstandswert – also teilen.
    let einstand = match h.acquisition_value {
        Some(wert) if stueck != 0.0 => dez(wert) / stueck,
        _ => 0.0,
    };
    PositionAus {
        name: h.name.clone(),
        isin: h.isin.as_ref().map(|i| i.as_str().to_string()).unwrap_or_default(),
        wkn: h.wkn.as_ref().map(|w| w.as_str().to_string()).unwrap_or_default(),
        symbol: String::new(), // FinTS kennt keine Börsenkürzel
        art: art_raten(&h.name, h.isin.as_ref().map(|i| i.as_str())),
        stueck,
        einstand,
        kurs: h.price.map(dez).unwrap_or(0.0),
        waehrung: h
            .price_currency
            .as_ref()
            .map(|c| c.as_str().to_string())
            .unwrap_or_else(|| "EUR".into()),
    }
}

/// Grobe Einordnung aus dem Namen – die Bank liefert keine Gattung mit.
/// Falsch geraten ist hier folgenlos: die Art steuert nur das Symbol in der
/// Liste, nicht die Rechnung.
fn art_raten(name: &str, isin: Option<&str>) -> String {
    let n = name.to_uppercase();
    if n.contains("ETF") || n.contains("UCITS") {
        "etf".into()
    } else if n.contains("FONDS") || n.contains("FUND") || n.contains("INVEST") {
        "fonds".into()
    } else if n.contains("ANLEIHE") || n.contains("BOND") || n.contains("OBLIGATION") {
        "anleihe".into()
    } else if isin.is_some() {
        "aktie".into()
    } else {
        "sonst".into()
    }
}
