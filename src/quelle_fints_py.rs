//! FinTS über `python-fints` – der Umweg, der tatsächlich funktioniert.
//!
//! `fints-rs` baut für ING und Commerzbank Nachrichten, die die Banken
//! zurückweisen: ING antwortet auf den zweiten Dialog mit
//! `9010 – Ungültiger Signaturaufbau: Fehler im Segmentaufbau`, die
//! Commerzbank schon auf den ersten mit `Die Nachricht enthält Fehler`.
//! Die Zugangsdaten und die Produkt-ID sind dabei nachweislich in Ordnung –
//! der Sync-Dialog bei ING läuft durch und liefert System-Kennung und
//! Bankparameter.
//!
//! Deshalb dasselbe Muster wie bei Trade Republic: der zerbrechliche Teil
//! bleibt bei einer gepflegten Fremdbibliothek, wir übersetzen nur deren
//! Ausgabe. `fints_helfer.py` gibt auf stdout genau ein JSON-Objekt in den
//! Formen der Brücke aus; alles Menschenlesbare geht auf stderr.
//!
//! Die PIN reisen wir über die Umgebung an, nicht über die Befehlszeile –
//! sonst stünde sie in der Prozessliste.

use crate::konfig::{FintsKonfig, Konto};
use crate::modell::{Bestand, KontoAus, PositionAus, UmsatzAus};
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::process::Stdio;

#[derive(Debug, Deserialize)]
struct HelferAusgabe {
    #[serde(default)]
    konto: Option<KontoAus>,
    #[serde(default)]
    umsaetze: Vec<UmsatzAus>,
    #[serde(default)]
    positionen: Vec<PositionAus>,
}

pub async fn abgleichen(
    konto: &Konto,
    fints: &FintsKonfig,
    bestand: &mut Bestand,
    interaktiv: bool,
) -> Result<String> {
    let heimat = crate::konfig::heimat();
    let skript = heimat.join("fints_helfer.py");
    if !skript.exists() {
        bail!("{} fehlt – ohne den Helfer geht FinTS über Python nicht.", skript.display());
    }

    let url = fints::bank_by_blz(&konto.blz)
        .map(|b| b.url.to_string())
        .ok_or_else(|| anyhow!("BLZ {} steht nicht im FinTS-Bankverzeichnis.", konto.blz))?;

    let zustand = crate::konfig::zustand_ordner().join(format!("{}-fints.bin", konto.id));

    let mut befehl = tokio::process::Command::new(&fints.python);
    befehl
        .arg(&skript)
        .arg(if interaktiv { "anmelden" } else { "abrufen" })
        .current_dir(&heimat)
        .env("FH_BLZ", &konto.blz)
        .env("FH_URL", &url)
        .env("FH_USER", &konto.benutzer)
        .env("FH_PIN", konto.pin())
        .env("FH_PRODUKT", &fints.produkt_id)
        .env("FH_IBAN", &konto.iban)
        .env("FH_ART", &konto.art)
        .env("FH_TAGE", konto.tage.to_string())
        .env("FH_STATE", zustand.display().to_string())
        .env("FH_REF", &konto.id)
        .env("FH_NAME", &konto.name)
        .env("FH_BANK", &konto.bank)
        // Python würde Umlaute sonst je nach Konsole unterschiedlich kodieren
        .env("PYTHONIOENCODING", "utf-8")
        .stdout(Stdio::piped())
        // stderr selbst mitlesen statt durchreichen: `inherit` verliert die
        // Ausgabe, sobald der Aufruf selbst umgeleitet wird oder als Dienst
        // läuft – dann stünde die eigentliche Fehlermeldung nirgends.
        .stderr(Stdio::piped())
        // Nur beim Anmelden darf der Helfer nach einer TAN fragen
        .stdin(if interaktiv { Stdio::inherit() } else { Stdio::null() });

    let mut kind = befehl.spawn().map_err(|e| {
        anyhow!(
            "`{}` lässt sich nicht starten ({e}). Python installieren oder \
             in der config.toml unter [fints] `python` auf den vollen Pfad setzen.",
            fints.python
        )
    })?;

    // Zeilenweise weiterreichen, damit die TAN-Aufforderung sofort sichtbar
    // ist und nicht erst, wenn der Helfer fertig ist.
    let strom = kind.stderr.take().expect("stderr ist gepiped");
    let mitlesen = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut zeilen = BufReader::new(strom).lines();
        let mut letzte: Vec<String> = Vec::new();
        while let Ok(Some(z)) = zeilen.next_line().await {
            eprintln!("{z}");
            letzte.push(z);
            if letzte.len() > 12 {
                letzte.remove(0);
            }
        }
        letzte
    });

    let mut roh = String::new();
    if let Some(mut aus) = kind.stdout.take() {
        use tokio::io::AsyncReadExt;
        aus.read_to_string(&mut roh).await.ok();
    }
    let status = kind.wait().await?;
    let letzte = mitlesen.await.unwrap_or_default();

    if !status.success() {
        bail!(
            "Der FinTS-Helfer ist ausgestiegen ({status}): {}",
            letzte.join(" / ")
        );
    }
    let erg: HelferAusgabe = serde_json::from_str(roh.trim())
        .with_context(|| format!("Antwort des Helfers nicht verstanden: {}", kurz(&roh)))?;

    let neu = bestand.umsaetze_zusammenfuehren(erg.umsaetze);
    if !erg.positionen.is_empty() {
        bestand.positionen = erg.positionen;
    }
    if let Some(k) = erg.konto {
        bestand.konto = Some(k);
    }
    bestand.stand = Some(chrono::Local::now().to_rfc3339());
    bestand.fehler = None;

    Ok(format!(
        "{}: {neu} neue Buchungen, {} Positionen{}",
        konto.name,
        bestand.positionen.len(),
        bestand
            .konto
            .as_ref()
            .map(|k| format!(", Saldo {:.2} {}", k.saldo, k.waehrung))
            .unwrap_or_default()
    ))
}

fn kurz(s: &str) -> String {
    s.chars().take(200).collect()
}
