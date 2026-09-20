//! Finanzhelfer-Brücke
//!
//! Ein kleiner Dienst auf dem eigenen Rechner, der mit den Banken spricht und
//! der Finanzhelfer-App genau die drei Endpunkte hinhält, die sie erwartet:
//!
//!   GET /konten                              → Konten mit Saldo
//!   GET /umsaetze?konto=<ref>&von=YYYY-MM-DD → Buchungen
//!   GET /positionen?depot=<ref>              → Depotbestände
//!
//! Alle drei verlangen `Authorization: Bearer <token>` aus der config.toml.
//!
//! Der Abruf bei der Bank läuft **nicht** im HTTP-Aufruf, sondern in einer
//! Schleife im Hintergrund; die Endpunkte liefern den zuletzt geholten Stand
//! aus `state/`. Ein Bankdialog dauert Sekunden, und Banken zählen mit –
//! eine App, die bei jedem Neuzeichnen eine Verbindung aufmacht, fliegt raus.

mod konfig;
mod modell;
mod quelle_bitvavo;
mod quelle_fints;
mod quelle_tr;

use anyhow::{Result, bail};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    routing::get,
};
use konfig::{Konfig, Konto, Quelle};
use modell::{Bestand, KontoAus, PositionAus, UmsatzAus};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{AllowOrigin, CorsLayer};

struct Lage {
    konfig: Konfig,
    staende: RwLock<HashMap<String, Bestand>>,
}

#[tokio::main]
async fn main() {
    if let Err(e) = los().await {
        eprintln!("\nFehler: {e:#}");
        std::process::exit(1);
    }
}

async fn los() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let befehl = args.first().map(String::as_str).unwrap_or("dienst");

    if matches!(befehl, "hilfe" | "--help" | "-h") {
        hilfe();
        return Ok(());
    }

    let konfig = Konfig::finden()?;

    // `pruefen` darf eine halbfertige Konfiguration ansehen – dafür ist es da.
    // Alle anderen Befehle brauchen eine vollständige.
    if befehl != "pruefen" {
        let offen = konfig.probleme();
        if !offen.is_empty() {
            eprintln!("Die Konfiguration ist noch nicht vollständig:\n");
            for x in &offen {
                eprintln!("  ✗ {x}");
            }
            eprintln!("\n`finanzhelfer-bruecke pruefen` zeigt den Stand jederzeit an.");
            std::process::exit(1);
        }
    }

    match befehl {
        "pruefen" => pruefen(&konfig).await,
        "anmelden" => {
            let Some(id) = args.get(1) else {
                bail!("Welches Konto? `finanzhelfer-bruecke anmelden <konto-id>`");
            };
            let konto = konfig
                .konto(id)
                .ok_or_else(|| anyhow::anyhow!("Kein Konto mit der Kennung `{id}` in der config.toml"))?;
            let meldung = abgleichen(konto, &konfig, true).await?;
            println!("✓ {meldung}");
            println!("\nDie Anmeldung ist gemerkt. Ab jetzt läuft der Abgleich ohne TAN,");
            println!("bis die Bank die Freigabe erneut verlangt (meist nach ~90 Tagen).");
            Ok(())
        }
        "abgleich" => {
            let ziel = args.get(1).map(String::as_str);
            let mut fehler = 0;
            for konto in &konfig.konten {
                if let Some(z) = ziel {
                    if konto.id != z {
                        continue;
                    }
                }
                match abgleichen(konto, &konfig, true).await {
                    Ok(m) => println!("✓ {m}"),
                    Err(e) => {
                        eprintln!("✗ {}: {e:#}", konto.name);
                        fehler += 1;
                    }
                }
            }
            if fehler > 0 {
                bail!("{fehler} Konten konnten nicht abgeglichen werden.");
            }
            Ok(())
        }
        "dienst" => dienst(konfig).await,
        anderes => {
            eprintln!("Unbekannter Befehl `{anderes}`.\n");
            hilfe();
            std::process::exit(2);
        }
    }
}

fn hilfe() {
    println!(
        "\
Finanzhelfer-Brücke {}

  finanzhelfer-bruecke pruefen           Konfiguration und Bankzugänge prüfen
  finanzhelfer-bruecke anmelden <konto>  Einmalige Anmeldung mit TAN-Freigabe
  finanzhelfer-bruecke abgleich [konto]  Einmal abrufen (alle oder eines)
  finanzhelfer-bruecke dienst            Server starten (Voreinstellung)

Die Zugangsdaten stehen in config.toml neben der Programmdatei.",
        env!("CARGO_PKG_VERSION")
    );
}

/// Zeigt, was die Konfiguration hergibt – ohne irgendwo anzuklopfen.
async fn pruefen(konfig: &Konfig) -> Result<()> {
    println!("Dienst lauscht auf   {}", konfig.dienst.adresse);
    println!("Abgleich alle        {} Minuten", konfig.dienst.abgleich_minuten);
    if let Some(ordner) = &konfig.dienst.app_ordner {
        let da = Path::new(ordner).join("index.html").exists();
        println!("App-Ordner           {ordner} {}", if da { "✓" } else { "✗ (keine index.html)" });
    }
    println!(
        "FinTS-Produkt-ID     {}",
        if konfig.fints.produkt_id.is_empty() { "– fehlt –" } else { &konfig.fints.produkt_id }
    );
    println!("\nKonten:");
    for k in &konfig.konten {
        let quelle = match k.quelle {
            Quelle::Fints => {
                match fints::bank_by_blz(&k.blz) {
                    Some(b) => format!("FinTS {} ({})", b.url, b.name),
                    None => format!("FinTS – BLZ {} steht nicht im Bankverzeichnis!", k.blz),
                }
            }
            Quelle::Pytr => "Trade Republic über pytr".into(),
            Quelle::Bitvavo => "Bitvavo (offizielle API)".into(),
        };
        let stand = Bestand::laden(&k.id);
        let zuletzt = stand.stand.clone().unwrap_or_else(|| "noch nie".into());
        println!("  {:<12} {:<28} {}", k.id, k.name, quelle);
        println!("  {:<12} {:<28} zuletzt: {zuletzt}", "", "");
        if stand.system_id.is_some() {
            println!("  {:<12} {:<28} Anmeldung gemerkt ✓", "", "");
        }
    }

    let offen = konfig.probleme();
    println!();
    if offen.is_empty() {
        println!("Alles beisammen. Als Nächstes für jedes FinTS-Konto einmal:");
        println!("  finanzhelfer-bruecke anmelden <konto-id>");
    } else {
        println!("Das fehlt noch:");
        for x in &offen {
            println!("  ✗ {x}");
        }
    }
    Ok(())
}

/// Ein Konto abgleichen und den Stand auf die Platte schreiben.
async fn abgleichen(konto: &Konto, konfig: &Konfig, interaktiv: bool) -> Result<String> {
    let mut bestand = Bestand::laden(&konto.id);
    let ergebnis = match konto.quelle {
        Quelle::Fints => {
            quelle_fints::abgleichen(konto, &konfig.fints.produkt_id, &mut bestand, interaktiv).await
        }
        Quelle::Bitvavo => quelle_bitvavo::abgleichen(konto, &mut bestand).await,
        Quelle::Pytr => quelle_tr::abgleichen(konto, &mut bestand).await,
    };

    match ergebnis {
        Ok(meldung) => {
            bestand.sichern(&konto.id)?;
            Ok(meldung)
        }
        Err(e) => {
            // Auch im Fehlerfall sichern: die System-Kennung aus einer
            // angefangenen FinTS-Anmeldung darf nicht verloren gehen.
            bestand.fehler = Some(format!("{e:#}"));
            bestand.sichern(&konto.id).ok();
            Err(e)
        }
    }
}

// ───────────────────────── Server ─────────────────────────

async fn dienst(konfig: Konfig) -> Result<()> {
    let adresse = konfig.dienst.adresse.clone();
    let takt = konfig.dienst.abgleich_minuten;
    let app_ordner = konfig.dienst.app_ordner.clone();

    // Den letzten bekannten Stand laden, damit die App sofort etwas bekommt
    let mut staende = HashMap::new();
    for k in &konfig.konten {
        staende.insert(k.id.clone(), Bestand::laden(&k.id));
    }

    let herkunft: Vec<header::HeaderValue> = konfig
        .dienst
        .herkunft
        .iter()
        .filter_map(|h| h.parse().ok())
        .collect();

    let lage = Arc::new(Lage { konfig, staende: RwLock::new(staende) });

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(herkunft))
        .allow_headers([header::AUTHORIZATION, header::ACCEPT, header::CONTENT_TYPE])
        .allow_methods([axum::http::Method::GET]);

    let mut app = Router::new()
        .route("/konten", get(konten))
        .route("/umsaetze", get(umsaetze))
        .route("/positionen", get(positionen))
        .route("/status", get(status))
        .layer(cors)
        .with_state(lage.clone());

    // Liegt die App daneben, liefern wir sie gleich mit aus. Dann sind App und
    // Brücke dieselbe Herkunft – kein CORS, keine Mixed-Content-Sperre, und
    // vom Handy aus erreichbar, sobald die Adresse auf 0.0.0.0 steht.
    if let Some(ordner) = &app_ordner {
        if Path::new(ordner).join("index.html").exists() {
            app = app.fallback_service(tower_http::services::ServeDir::new(ordner));
            println!("App wird mit ausgeliefert aus {ordner}");
        } else {
            eprintln!("! In {ordner} liegt keine index.html – die App wird nicht ausgeliefert.");
        }
    }

    if takt > 0 {
        let lage2 = lage.clone();
        tokio::spawn(async move { schleife(lage2, takt).await });
    }

    let horcher = tokio::net::TcpListener::bind(&adresse).await?;
    println!("Finanzhelfer-Brücke läuft auf http://{adresse}");
    println!("Zum Beenden Strg+C.");
    axum::serve(horcher, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            println!("\nAuf Wiedersehen.");
        })
        .await?;
    Ok(())
}

/// Regelmäßiger Abgleich im Hintergrund.
async fn schleife(lage: Arc<Lage>, minuten: u64) {
    // Beim Start kurz warten: erst soll die App antworten können
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    loop {
        for konto in &lage.konfig.konten {
            match abgleichen(konto, &lage.konfig, false).await {
                Ok(m) => println!("  {m}"),
                Err(e) => eprintln!("  ! {}: {e:#}", konto.name),
            }
            // Den frischen Stand in den Arbeitsspeicher übernehmen
            let neu = Bestand::laden(&konto.id);
            lage.staende.write().await.insert(konto.id.clone(), neu);
        }
        tokio::time::sleep(std::time::Duration::from_secs(minuten * 60)).await;
    }
}

// ───────────────────────── Endpunkte ─────────────────────────

type Antwort<T> = std::result::Result<Json<T>, (StatusCode, String)>;

fn token_pruefen(lage: &Lage, kopf: &HeaderMap) -> std::result::Result<(), (StatusCode, String)> {
    let mitgeschickt = kopf
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .trim();
    if mitgeschickt == lage.konfig.dienst.token {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "Falsches oder fehlendes Token".into()))
    }
}

#[derive(Debug, serde::Deserialize)]
struct Filter {
    konto: Option<String>,
    depot: Option<String>,
    von: Option<String>,
}

async fn konten(State(lage): State<Arc<Lage>>, kopf: HeaderMap) -> Antwort<Vec<KontoAus>> {
    token_pruefen(&lage, &kopf)?;
    let staende = lage.staende.read().await;
    // Depots stehen bewusst nicht drin – die holt die App über /positionen
    let liste = lage
        .konfig
        .konten
        .iter()
        .filter(|k| !k.ist_depot())
        .filter_map(|k| staende.get(&k.id).and_then(|b| b.konto.clone()))
        .collect();
    Ok(Json(liste))
}

async fn umsaetze(
    State(lage): State<Arc<Lage>>,
    Query(filter): Query<Filter>,
    kopf: HeaderMap,
) -> Antwort<Vec<UmsatzAus>> {
    token_pruefen(&lage, &kopf)?;
    let staende = lage.staende.read().await;
    let mut raus = Vec::new();
    for (id, bestand) in staende.iter() {
        if let Some(gewuenscht) = &filter.konto {
            if id != gewuenscht {
                continue;
            }
        }
        for u in &bestand.umsaetze {
            if let Some(von) = &filter.von {
                if u.datum.as_str() < von.as_str() {
                    continue;
                }
            }
            raus.push(u.clone());
        }
    }
    raus.sort_by(|a, b| b.datum.cmp(&a.datum));
    Ok(Json(raus))
}

async fn positionen(
    State(lage): State<Arc<Lage>>,
    Query(filter): Query<Filter>,
    kopf: HeaderMap,
) -> Antwort<Vec<PositionAus>> {
    token_pruefen(&lage, &kopf)?;
    let staende = lage.staende.read().await;
    let mut raus = Vec::new();
    for (id, bestand) in staende.iter() {
        // Leerer Depot-Filter heißt „alles" – so fragt die App, wenn beim
        // Depot keine Kennung hinterlegt ist
        if let Some(gewuenscht) = &filter.depot {
            if !gewuenscht.is_empty() && id != gewuenscht {
                continue;
            }
        }
        raus.extend(bestand.positionen.iter().cloned());
    }
    Ok(Json(raus))
}

#[derive(Serialize)]
struct StatusZeile {
    konto: String,
    name: String,
    quelle: String,
    stand: Option<String>,
    umsaetze: usize,
    positionen: usize,
    angemeldet: bool,
    fehler: Option<String>,
}

async fn status(State(lage): State<Arc<Lage>>, kopf: HeaderMap) -> Antwort<Vec<StatusZeile>> {
    token_pruefen(&lage, &kopf)?;
    let staende = lage.staende.read().await;
    let liste = lage
        .konfig
        .konten
        .iter()
        .map(|k| {
            let b = staende.get(&k.id);
            StatusZeile {
                konto: k.id.clone(),
                name: k.name.clone(),
                quelle: format!("{:?}", k.quelle).to_lowercase(),
                stand: b.and_then(|b| b.stand.clone()),
                umsaetze: b.map(|b| b.umsaetze.len()).unwrap_or(0),
                positionen: b.map(|b| b.positionen.len()).unwrap_or(0),
                angemeldet: b.map(|b| b.system_id.is_some()).unwrap_or(false),
                fehler: b.and_then(|b| b.fehler.clone()),
            }
        })
        .collect();
    Ok(Json(liste))
}
