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

#[derive(Debug, Deserialize)]
pub struct FintsKonfig {
    /// Produkt-Registrierungsnummer der Deutschen Kreditwirtschaft.
    /// Ohne sie weisen die meisten Banken den Zugang ab – siehe README.
    #[serde(default)]
    pub produkt_id: String,
    /// Welcher FinTS-Motor spricht mit der Bank?
    ///
    /// `python` (Voreinstellung) ruft `fints_helfer.py` auf und damit
    /// `python-fints`. `rust` nimmt die eingebaute Bibliothek `fints-rs` –
    /// die ist schlanker, baut aber für ING und Commerzbank Nachrichten,
    /// die diese Banken zurückweisen. Siehe quelle_fints_py.rs.
    #[serde(default = "standard_motor")]
    pub motor: String,
    /// Womit Python gestartet wird. Voller Pfad hilft, wenn die Umgebung
    /// eines Dienstes den PATH nicht kennt.
    #[serde(default = "standard_python")]
    pub python: String,
}

impl Default for FintsKonfig {
    fn default() -> Self {
        Self { produkt_id: String::new(), motor: standard_motor(), python: standard_python() }
    }
}

fn standard_motor() -> String { "python".into() }
fn standard_python() -> String { "python".into() }

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
    /// FinTS-Adresse der Bank. Leer = im Bankverzeichnis nachschlagen.
    ///
    /// Nötig, wenn das Verzeichnis zur eigenen BLZ keine Adresse kennt: die
    /// Commerzbank etwa bedient all ihre Bankleitzahlen über **einen** Server,
    /// eingetragen ist er aber nur bei einer davon.
    #[serde(default)]
    pub url: String,
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

/// Wo nach der `config.toml` gesucht wird – in dieser Reihenfolge:
///   1. im aktuellen Verzeichnis (so arbeitet man beim Entwickeln)
///   2. neben der Programmdatei (so startet man sie per Doppelklick oder
///      Aufgabenplanung, und dann ist das Arbeitsverzeichnis irgendwas)
///   3. eine Ebene über der Programmdatei, wegen `target\release\`
pub fn konfig_orte() -> Vec<PathBuf> {
    let mut orte = vec![PathBuf::from("config.toml")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(ordner) = exe.parent() {
            orte.push(ordner.join("config.toml"));
            if let Some(darueber) = ordner.parent() {
                orte.push(darueber.join("config.toml"));
                // target\release\ -> Projektordner
                if let Some(nochmal) = darueber.parent() {
                    orte.push(nochmal.join("config.toml"));
                }
            }
        }
    }
    orte
}

impl Konfig {
    /// Sucht die Konfiguration an allen plausiblen Stellen.
    pub fn finden() -> Result<Self> {
        let orte = konfig_orte();
        for ort in &orte {
            if ort.is_file() {
                return Self::laden(ort);
            }
        }
        bail!(
            "Keine config.toml gefunden. Gesucht wurde in:\n{}\n\n\
             Lege sie nach dem Muster von config.beispiel.toml an:\n  \
             copy config.beispiel.toml config.toml",
            orte.iter().map(|o| format!("  - {}", o.display())).collect::<Vec<_>>().join("\n")
        );
    }

    pub fn laden(pfad: &Path) -> Result<Self> {
        let roh = std::fs::read_to_string(pfad)
            .with_context(|| format!("{} lässt sich nicht lesen", pfad.display()))?;
        let konfig: Konfig = toml::from_str(&roh)
            .with_context(|| format!("{} ist kein gültiges TOML", pfad.display()))?;
        let heimat = pfad.parent().filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        HEIMAT.set(heimat).ok();
        println!("Konfiguration: {}", pfad.display());
        Ok(konfig)
    }

    /// Was unabhängig von einzelnen Konten fehlt.
    pub fn probleme_allgemein(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.dienst.token.trim().is_empty() || self.dienst.token.starts_with("hier-eine-lange") {
            p.push("[dienst] `token` fehlt noch – denk dir eine lange, zufällige Zeichenkette aus.".into());
        }
        if self.konten.is_empty() {
            p.push("Es ist kein einziges [[konto]] eingetragen.".into());
        }
        let mut gesehen = std::collections::HashSet::new();
        for k in &self.konten {
            if !gesehen.insert(&k.id) {
                p.push(format!("Die Konto-Kennung `{}` kommt zweimal vor – sie muss eindeutig sein.", k.id));
            }
        }
        p
    }

    /// Was **diesem einen** Konto fehlt.
    ///
    /// Bewusst je Konto: ein unvollständiges Konto darf die anderen nicht
    /// blockieren. Wer Trade Republic einrichtet, soll das nicht lassen
    /// müssen, weil die FinTS-Produkt-ID noch bei der Post liegt.
    pub fn probleme_konto(&self, k: &Konto) -> Vec<String> {
        let mut p = Vec::new();
        match k.quelle {
            Quelle::Fints => {
                if self.fints.produkt_id.trim().is_empty() {
                    p.push(
                        "[fints] `produkt_id` fehlt. Ohne registrierte Produkt-ID weisen die Banken \
                         FinTS-Zugriffe ab – Formular unter fints.org/de/hersteller/produktregistrierung."
                            .into(),
                    );
                }
                for (feld, wert) in [("blz", &k.blz), ("iban", &k.iban), ("benutzer", &k.benutzer)] {
                    if wert.trim().is_empty() || wert.contains("00000000") {
                        p.push(format!("Konto `{}`: `{feld}` fehlt noch.", k.id));
                    }
                }
                if k.pin().is_empty() {
                    p.push(format!(
                        "Konto `{}`: keine PIN. Entweder in die Datei eintragen oder die \
                         Umgebungsvariable FH_PIN_{} setzen.",
                        k.id,
                        k.id.to_uppercase().replace('-', "_")
                    ));
                }
            }
            Quelle::Bitvavo => {
                if k.schluessel.trim().is_empty() || k.geheimnis.trim().is_empty() {
                    p.push(format!(
                        "Konto `{}`: `schluessel` und `geheimnis` fehlen (API-Key mit Leserecht).",
                        k.id
                    ));
                }
            }
            Quelle::Pytr => {}
        }
        p
    }

    /// Alles zusammen – für den Merkzettel in `pruefen`.
    pub fn probleme(&self) -> Vec<String> {
        let mut p = self.probleme_allgemein();
        for k in &self.konten {
            p.extend(self.probleme_konto(k));
        }
        p.dedup();
        p
    }

    pub fn konto(&self, id: &str) -> Option<&Konto> {
        self.konten.iter().find(|k| k.id == id)
    }
}

/// Wo die Konfiguration gefunden wurde. `state/` landet daneben – sonst
/// schriebe ein per Aufgabenplanung gestarteter Dienst seinen Bestand
/// irgendwohin und fände ihn beim nächsten Start nicht wieder.
static HEIMAT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Der Ordner, in dem die Konfiguration gefunden wurde. Dort liegen auch
/// `fints_helfer.py` und `state/`.
pub fn heimat() -> PathBuf {
    HEIMAT.get().cloned().unwrap_or_else(|| PathBuf::from("."))
}

/// Ablageort für Sitzungsdaten und den zuletzt geholten Bestand.
pub fn zustand_ordner() -> PathBuf {
    HEIMAT
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("state")
}
