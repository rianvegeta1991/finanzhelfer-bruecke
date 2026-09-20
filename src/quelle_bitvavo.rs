//! Bitvavo – hier gibt es eine **offizielle** Schnittstelle, kein Nachbau.
//!
//! Gebraucht wird ein API-Schlüssel, den du in deinem Bitvavo-Konto anlegst.
//! Vergib dabei **nur das Leserecht** („View"): die Brücke muss Bestände
//! lesen, sie soll nichts handeln können. Ein Schlüssel ohne Handelsrecht
//! kann im schlimmsten Fall auch nichts anrichten.
//!
//! Signiert wird mit HMAC-SHA256 über
//! `Zeitstempel + Methode + Pfad (mit /v2) + Rumpf`.

use crate::konfig::Konto;
use crate::modell::{Bestand, PositionAus};
use anyhow::{Context, Result, anyhow};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

const BASIS: &str = "https://api.bitvavo.com/v2";

#[derive(Debug, Deserialize)]
struct Guthaben {
    symbol: String,
    available: String,
    #[serde(rename = "inOrder")]
    in_order: String,
}

#[derive(Debug, Deserialize)]
struct Kurs {
    market: String,
    price: String,
}

fn signieren(geheimnis: &str, zeit: i64, methode: &str, pfad: &str) -> Result<String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(geheimnis.as_bytes())
        .map_err(|e| anyhow!("Bitvavo-Geheimnis unbrauchbar: {e}"))?;
    // Rumpf ist bei GET die leere Zeichenkette und fällt damit weg
    mac.update(format!("{zeit}{methode}{pfad}").as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

async fn holen<T: serde::de::DeserializeOwned>(
    klient: &reqwest::Client,
    konto: &Konto,
    pfad: &str,
) -> Result<T> {
    let zeit = chrono::Utc::now().timestamp_millis();
    let voll = format!("/v2{pfad}");
    let signatur = signieren(&konto.geheimnis, zeit, "GET", &voll)?;

    let antwort = klient
        .get(format!("{BASIS}{pfad}"))
        .header("Bitvavo-Access-Key", &konto.schluessel)
        .header("Bitvavo-Access-Timestamp", zeit.to_string())
        .header("Bitvavo-Access-Signature", signatur)
        .header("Bitvavo-Access-Window", "10000")
        .send()
        .await
        .with_context(|| format!("Bitvavo {pfad} nicht erreichbar"))?;

    let status = antwort.status();
    let text = antwort.text().await.unwrap_or_default();
    if !status.is_success() {
        // Bitvavo schickt den Grund im Rumpf mit – der ist hier mehr wert
        // als der nackte Statuscode
        return Err(anyhow!("Bitvavo antwortet mit {status}: {}", text.trim()));
    }
    serde_json::from_str(&text)
        .with_context(|| format!("Bitvavo-Antwort auf {pfad} nicht verstanden: {}", kurz(&text)))
}

fn kurz(s: &str) -> String {
    s.chars().take(160).collect()
}

pub async fn abgleichen(konto: &Konto, bestand: &mut Bestand) -> Result<String> {
    let klient = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;

    let guthaben: Vec<Guthaben> = holen(&klient, konto, "/balance").await?;

    // Die Kursliste ist öffentlich und braucht keine Signatur
    let kurse: Vec<Kurs> = klient
        .get(format!("{BASIS}/ticker/price"))
        .send()
        .await
        .context("Bitvavo-Kursliste nicht erreichbar")?
        .json()
        .await
        .context("Bitvavo-Kursliste nicht verstanden")?;

    let kurs_von = |symbol: &str| -> f64 {
        let markt = format!("{symbol}-EUR");
        kurse
            .iter()
            .find(|k| k.market == markt)
            .and_then(|k| k.price.parse::<f64>().ok())
            .unwrap_or(0.0)
    };

    let mut positionen = Vec::new();
    for g in &guthaben {
        let menge = g.available.parse::<f64>().unwrap_or(0.0) + g.in_order.parse::<f64>().unwrap_or(0.0);
        if menge <= 0.0 {
            continue;
        }
        if g.symbol == "EUR" {
            // Das Euro-Guthaben ist kein Wertpapier, gehört aber zum Depotwert.
            // Als Position mit Kurs 1 bleibt die Summe unten richtig.
            positionen.push(PositionAus {
                name: format!("Euro-Guthaben ({})", konto.name),
                isin: String::new(),
                wkn: String::new(),
                symbol: String::new(),
                art: "sonst".into(),
                stueck: menge,
                einstand: 1.0,
                kurs: 1.0,
                waehrung: "EUR".into(),
            });
            continue;
        }
        positionen.push(PositionAus {
            name: g.symbol.clone(),
            isin: String::new(),
            wkn: String::new(),
            // Das Kürzel reicht der App, um den Kurs selbst über CoinGecko
            // nachzuladen, falls die Brücke gerade nicht läuft
            symbol: g.symbol.clone(),
            art: "krypto".into(),
            stueck: menge,
            // Bitvavo liefert über /balance keinen Einstandswert. 0 heißt für
            // die App „unbekannt" – ein von Hand gepflegter Wert bleibt stehen.
            einstand: 0.0,
            kurs: kurs_von(&g.symbol),
            waehrung: "EUR".into(),
        });
    }

    let anzahl = positionen.len();
    let summe: f64 = positionen.iter().map(|p| p.stueck * p.kurs).sum();
    bestand.positionen = positionen;
    bestand.stand = Some(chrono::Local::now().to_rfc3339());
    bestand.fehler = None;

    Ok(format!(
        "{}: {anzahl} Positionen, zusammen {summe:.2} EUR",
        konto.name
    ))
}
