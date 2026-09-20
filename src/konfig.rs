//! Konfiguration aus `config.toml`.
//!
//! Die Datei enthält Zugangsdaten und gehört deshalb **nicht** ins Repo –
//! `.gitignore` hält sie draußen. Wer die PIN nicht im Klartext ablegen will,
//! lässt das Feld leer und setzt stattdessen die Umgebungsvariable
//! `FH_PIN_<KONTO-ID in Großbuchstaben, Bindestrich zu Unterstrich>`.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Konfig {
    pub dienst: Dienst,
    #[serde(default)]
    pub fints: FintsKonfig,
    /// In der TOML-Datei als wiederholte `[[konto]]`-Abschnitte.
    #[serde(default, rename = "konto")]
    pub konten: Vec<Konto>,
}

#[derive(Debug, Deserialize)]
pub struct Dienst {
    /// Worauf gelauscht wird. `127.0.0.1:8123` ist nur vom eigenen Rechner
    /// erreichbar; `0.0.0.0:8123` öffnet die Brücke fürs ganze WLAN.
    #[serde(default = "standard_adresse")]
    pub adresse: String,
    /// Frei erfundenes Geheimnis. Muss im Finanzhelfer unter
    /// „Automatischer Abruf → Token" genauso eingetragen werden.
    pub token: String,
    /// Optional: Ordner mit der Finanzhelfer-App. Ist er gesetzt, liefert die
    /// Brücke die App gleich mit aus – dann liegen App und Brücke auf derselben
    /// Herkunft, und CORS spielt gar keine Rolle mehr.
    #[serde(default)]
    pub app_ordner: Option<String>,
    /// Welche fremden Herkünfte die Brücke abfragen dürfen.
    #[serde(default = "standard_herkunft")]
    pub herkunft: Vec<String>,
    /// Abstand zwischen zwei selbsttätigen Abgleichen. 0 schaltet sie ab.
    #[serde(default = "standard_takt")]
    pub abgleich_minuten: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct FintsKonfig {
    /// Produkt-Registrierungsnummer der Deutschen Kreditwirtschaft.
    /// Ohne sie weisen die meisten Banken den Zugang ab – siehe README.
    #[serde(default)]
    pub produkt_id: String,
}

/// Woher die Daten eines Kontos kommen.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Quelle {
    /// FinTS/HBCI direkt bei der Bank (ING, Commerzbank, Sparkassen …).
    Fints,
    /// Trade Republic über das Fremdwerkzeug `pytr`.
    Pytr,
    /// Bitvavo über die offizielle REST-Schnittstelle.
    Bitvavo,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Konto {
    /// Kurze, stabile Kennung. Sie ist zugleich die `ref`, die der
    /// Finanzhelfer beim Konto unter „Kennung bei der Brücke" erwartet.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub bank: String,
    /// `giro`, `tagesgeld`, `kreditkarte`, `kredit`, `bar` – oder `depot`.
    #[serde(default = "standard_art")]
    pub art: String,
    pub quelle: Quelle,

    // ---- nur für FinTS ----
    #[serde(default)]
    pub blz: String,
    #[serde(default)]
    pub iban: String,
    #[serde(default)]
    pub bic: String,
    /// Zugangsnummer/Benutzerkennung im Online-Banking.
    #[serde(default)]
    pub benutzer: String,
    #[serde(default)]
    pub pin: String,
    /// Wie weit zurück Umsätze geholt werden.
    #[serde(default = "standard_tage")]
    pub tage: u32,

    // ---- nur für Bitvavo ----
    #[serde(default)]
    pub schluessel: String,
    #[serde(default)]
    pub geheimnis: String,

    // ---- nur für pytr ----
    /// Ordner, in den `pytr` seine Dateien schreibt. Leer = `state/<id>/pytr`.
    #[serde(default)]
    pub pytr_ordner: String,
    /// Wie pytr gestartet wird. Leer = `python -m pytr`, was immer geht;
    /// wer `pytr.exe` im PATH hat, trägt hier einfach `pytr` ein.
    #[serde(default)]
    pub pytr_programm: String,
    /// Zusätzliche Schalter für jeden pytr-Aufruf, z. B. `["--v2"]` für
    /// Konten, die die Freigabe über die TR-App statt per SMS machen.
    #[serde(default)]
    pub pytr_argumente: Vec<String>,
}

fn standard_adresse() -> String { "127.0.0.1:8123".into() }
fn standard_herkunft() -> Vec<String> {
    vec![
        "http://localhost:8797".into(),
        "http://127.0.0.1:8797".into(),
        "https://rianvegeta1991.github.io".into(),
    ]
}
fn standard_takt() -> u64 { 180 }
fn standard_art() -> String { "giro".into() }
fn standard_tage() -> u32 { 90 }

impl Konto {
    /// Die PIN – aus der Datei oder aus der Umgebung.
    pub fn pin(&self) -> String {
        if !self.pin.is_empty() {
            return self.pin.clone();
        }
        let name = format!("FH_PIN_{}", self.id.to_uppercase().replace('-', "_"));
        std::env::var(&name).unwrap_or_default()
    }

    pub fn ist_depot(&self) -> bool {
        self.art == "depot"
    }

    /// Programm und Vorlauf-Argumente für pytr.
    ///
    /// `python -m pytr` als Voreinstellung, weil der Scripts-Ordner von Python
    /// unter Windows oft nicht im PATH steht – `pytr.exe` liegt dann zwar da,
    /// lässt sich aber nicht ohne vollen Pfad aufrufen.
    pub fn pytr_befehl(&self) -> (String, Vec<String>) {
        if self.pytr_programm.is_empty() {
            ("python".into(), vec!["-m".into(), "pytr".into()])
        } else {
            (self.pytr_programm.clone(), Vec::new())
        }
    }
}

impl Konfig {
    pub fn laden(pfad: &Path) -> Result<Self> {
        let roh = std::fs::read_to_string(pfad)
            .with_context(|| format!("{} lässt sich nicht lesen. Lege sie nach dem Muster von config.beispiel.toml an.", pfad.display()))?;
        let konfig: Konfig = toml::from_str(&roh)
            .with_context(|| format!("{} ist kein gültiges TOML", pfad.display()))?;
        konfig.pruefen()?;
        Ok(konfig)
    }

    /// Früh meckern statt später im Bankdialog scheitern.
    fn pruefen(&self) -> Result<()> {
        if self.dienst.token.trim().is_empty() {
            bail!("In [dienst] fehlt ein `token`. Denk dir eine lange, zufällige Zeichenkette aus.");
        }
        if self.konten.is_empty() {
            bail!("Es ist kein einziges [[konto]] eingetragen.");
        }
        let mut gesehen = std::collections::HashSet::new();
        for k in &self.konten {
            if !gesehen.insert(&k.id) {
                bail!("Die Konto-Kennung `{}` kommt zweimal vor – sie muss eindeutig sein.", k.id);
            }
            match k.quelle {
                Quelle::Fints => {
                    if k.blz.is_empty() || k.iban.is_empty() || k.benutzer.is_empty() {
                        bail!("Konto `{}`: für FinTS braucht es `blz`, `iban` und `benutzer`.", k.id);
                    }
                    if k.pin().is_empty() {
                        bail!(
                            "Konto `{}`: keine PIN. Trag sie in die Datei ein oder setze die Umgebungsvariable FH_PIN_{}.",
                            k.id,
                            k.id.to_uppercase().replace('-', "_")
                        );
                    }
                }
                Quelle::Bitvavo => {
                    if k.schluessel.is_empty() || k.geheimnis.is_empty() {
                        bail!("Konto `{}`: für Bitvavo braucht es `schluessel` und `geheimnis` (API-Key mit Leserecht).", k.id);
                    }
                }
                Quelle::Pytr => {}
            }
        }
        if self.konten.iter().any(|k| k.quelle == Quelle::Fints) && self.fints.produkt_id.trim().is_empty() {
            eprintln!(
                "! Achtung: keine `produkt_id` unter [fints]. Die meisten Banken weisen FinTS-Zugriffe\n\
                 !          ohne registrierte Produkt-ID ab. Wie man sie bekommt, steht im README."
            );
        }
        Ok(())
    }

    pub fn konto(&self, id: &str) -> Option<&Konto> {
        self.konten.iter().find(|k| k.id == id)
    }
}

/// Ablageort für Sitzungsdaten und den zuletzt geholten Bestand.
pub fn zustand_ordner() -> PathBuf {
    PathBuf::from("state")
}
