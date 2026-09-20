//! Die Formen, die über die Leitung gehen – genau die, die der Finanzhelfer
//! erwartet (siehe `banking.js` in der App). Diese drei Strukturen sind der
//! Vertrag zwischen Brücke und App und dürfen nicht stillschweigend wandern.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// `GET /konten`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KontoAus {
    /// Die Kennung, die die App beim Konto hinterlegt.
    #[serde(rename = "ref")]
    pub kennung: String,
    pub name: String,
    pub bank: String,
    pub iban: String,
    /// `giro`, `tagesgeld`, `kreditkarte`, `kredit`, `bar`
    pub art: String,
    pub saldo: f64,
    pub waehrung: String,
}

/// `GET /umsaetze?konto=…&von=…` – negativer Betrag heißt Abbuchung.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UmsatzAus {
    pub datum: String,
    pub betrag: f64,
    pub gegen: String,
    pub zweck: String,
    pub waehrung: String,
}

/// `GET /positionen?depot=…`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionAus {
    pub name: String,
    pub isin: String,
    pub wkn: String,
    /// Börsenkürzel für den Kursabruf der App. FinTS liefert keines mit;
    /// die App kommt aber auch mit dem hier gelieferten `kurs` aus.
    pub symbol: String,
    /// `aktie`, `etf`, `fonds`, `anleihe`, `krypto`, `sonst`
    pub art: String,
    pub stueck: f64,
    pub einstand: f64,
    pub kurs: f64,
    pub waehrung: String,
}

/// Was die Brücke je Konto zuletzt geholt hat. Liegt als
/// `state/<konto-id>.json` auf der Platte, damit ein Neustart nicht jedes Mal
/// einen Bankdialog auslöst – FinTS ist langsam, und Banken zählen mit.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Bestand {
    pub konto: Option<KontoAus>,
    #[serde(default)]
    pub umsaetze: Vec<UmsatzAus>,
    #[serde(default)]
    pub positionen: Vec<PositionAus>,
    /// FinTS-Systemkennung. Sie ist der Grund, warum nicht jeder Abruf eine
    /// TAN verlangt: mit gemerkter Kennung erkennt die Bank das Gerät wieder.
    #[serde(default)]
    pub system_id: Option<String>,
    /// Zeitpunkt des letzten erfolgreichen Abgleichs (RFC 3339).
    #[serde(default)]
    pub stand: Option<String>,
    /// Was beim letzten Versuch schiefging – für `GET /status`.
    #[serde(default)]
    pub fehler: Option<String>,
}

impl Bestand {
    fn pfad(konto_id: &str) -> PathBuf {
        crate::konfig::zustand_ordner().join(format!("{konto_id}.json"))
    }

    /// Fehlt die Datei, ist das kein Fehler – dann war eben noch kein Abgleich.
    pub fn laden(konto_id: &str) -> Self {
        let pfad = Self::pfad(konto_id);
        match std::fs::read_to_string(&pfad) {
            Ok(roh) => serde_json::from_str(&roh).unwrap_or_else(|e| {
                eprintln!("! {} ist beschädigt ({e}) – fange von vorn an.", pfad.display());
                Bestand::default()
            }),
            Err(_) => Bestand::default(),
        }
    }

    pub fn sichern(&self, konto_id: &str) -> Result<()> {
        let ordner = crate::konfig::zustand_ordner();
        std::fs::create_dir_all(&ordner)
            .with_context(|| format!("{} lässt sich nicht anlegen", ordner.display()))?;
        let pfad = Self::pfad(konto_id);
        let roh = serde_json::to_string_pretty(self)?;
        std::fs::write(&pfad, roh).with_context(|| format!("{} lässt sich nicht schreiben", pfad.display()))?;
        Ok(())
    }

    /// Neue Umsätze dazulegen, ohne Doppelte. Banken buchen rückwirkend nach,
    /// deshalb wird bei jedem Abruf ein Stück Vergangenheit mitgeholt.
    pub fn umsaetze_zusammenfuehren(&mut self, neue: Vec<UmsatzAus>) -> usize {
        let mut bekannt: std::collections::HashSet<String> =
            self.umsaetze.iter().map(signatur).collect();
        let mut dazu = 0;
        for u in neue {
            let sig = signatur(&u);
            if bekannt.insert(sig) {
                self.umsaetze.push(u);
                dazu += 1;
            }
        }
        // Neueste zuerst – die App sortiert selbst noch einmal, aber so ist
        // die Datei auch von Hand lesbar
        self.umsaetze.sort_by(|a, b| b.datum.cmp(&a.datum));
        dazu
    }
}

/// Derselbe Fingerabdruck wie in der App: Kontoauszüge tragen keine stabile ID.
fn signatur(u: &UmsatzAus) -> String {
    let kurz = |s: &str, n: usize| s.chars().take(n).collect::<String>().to_lowercase();
    format!(
        "{}|{:.2}|{}|{}",
        u.datum,
        u.betrag,
        kurz(&u.gegen, 24),
        kurz(&u.zweck, 40)
    )
}

/// `rust_decimal::Decimal` → f64 für die Ausgabe. Die App rechnet ohnehin in
/// JavaScript-Zahlen; bei Beträgen dieser Größenordnung ist das verlustfrei.
pub fn dez(d: rust_decimal::Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}
