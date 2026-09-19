//! Belize national wastewater biosurveillance simulation.
//!
//! # Network topology
//!
//! ```text
//!   [orange_walk]          [san_ignacio]
//!        │                       │
//!        ▼                       ▼
//!   [bcity_north]  ←──    [bcity_wwtp]  ←── [belmopan_core]
//!        │                   ▲                [belmopan_ind]
//!   [bcity_south] ───────────┘
//! ```
//!
//! Catchment zones (hyperedges):
//! * **Belize City Trunk** — north, south, wwtp  
//! * **Belmopan Zone** — core, industrial  
//! * **Northern District** — orange_walk  
//! * **Western Highlands** — san_ignacio  
//!
//! # Outbreak scenario (15 simulation days)
//!
//! | Day | Event |
//! |-----|-------|
//! | 0–7 | Baseline warm-up (all pathogens at endemic background) |
//! | 8   | SARS-CoV-2 starts rising at Belize City South (AMBER) |
//! | 9   | SARS-CoV-2 peak at South; network spread to North → RED then CRITICAL |
//! | 11  | Mpox traveller import at Belmopan Core (AMBER) |
//! | 12+ | Signals gradually return to baseline |

use std::collections::HashMap;

use ontology_engine::prelude::OntologyEngine;

use ww_audit::{AuditAction, AuditLog};
use ww_detection::{AnomalyDetector, SewageNetwork};
use ww_domain::{
    factory::{AlertParams, DomainFactory, SampleParams, SignalParams, SiteParams},
    Result as DomainResult,
};

// ── Site definitions ─────────────────────────────────────────────────────────

struct Site {
    id:             &'static str,
    name:           &'static str,
    region:         &'static str,
    catchment_pop:  i64,
    lat:            f64,
    lon:            f64,
}

const SITES: &[Site] = &[
    Site { id: "bcity_north", name: "Belize City North Side",  region: "Belize District", catchment_pop: 28_000, lat:  17.510, lon: -88.189 },
    Site { id: "bcity_south", name: "Belize City South Side",  region: "Belize District", catchment_pop: 24_000, lat:  17.488, lon: -88.194 },
    Site { id: "bcity_wwtp",  name: "Belize City WWTP Inlet",  region: "Belize District", catchment_pop: 72_000, lat:  17.499, lon: -88.185 },
    Site { id: "belmopan_core", name: "Belmopan Urban Core",   region: "Cayo District",   catchment_pop: 16_000, lat:  17.252, lon: -88.768 },
    Site { id: "belmopan_ind",  name: "Belmopan Industrial Zone", region: "Cayo District", catchment_pop:  4_000, lat:  17.245, lon: -88.762 },
    Site { id: "orange_walk",   name: "Orange Walk Town",      region: "Orange Walk District", catchment_pop: 14_000, lat: 18.090, lon: -88.560 },
    Site { id: "san_ignacio",   name: "San Ignacio/Santa Elena", region: "Cayo District", catchment_pop: 18_000, lat: 17.157, lon: -89.072 },
    Site { id: "dangriga",      name: "Dangriga Town",         region: "Stann Creek District", catchment_pop: 11_000, lat: 16.971, lon: -88.234 },
];

// ── Pathogens ────────────────────────────────────────────────────────────────

struct Pathogen {
    name:         &'static str,
    gene:         &'static str,
    method:       &'static str,
    /// Background log₁₀ copies/L (endemic baseline).
    baseline_log: f64,
    /// Noise std-dev in log₁₀ space.
    noise_std:    f64,
}

const PATHOGENS: &[Pathogen] = &[
    Pathogen { name: "SARS-CoV-2",          gene: "N1",   method: "qPCR",   baseline_log: 3.8, noise_std: 0.22 },
    Pathogen { name: "Mpox",                gene: "E6L",  method: "ddPCR",  baseline_log: 1.0, noise_std: 0.18 },
    Pathogen { name: "Vibrio_cholerae",     gene: "ctxA", method: "qPCR",   baseline_log: 2.2, noise_std: 0.20 },
];

// ── Pseudo-random number generator (no external dep) ─────────────────────────

fn lcg(seed: u64) -> u64 {
    seed.wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407)
}

/// Approximate standard-normal sample using 12-uniform sum (CLT).
fn approx_normal(seed: u64) -> f64 {
    let mut x = 0.0f64;
    let mut s = seed;
    for _ in 0..12 {
        s = lcg(s);
        x += (s as f64) / (u64::MAX as f64);
    }
    x - 6.0   // E[sum]=6, Var[sum]=1
}

// ── Outbreak signals ─────────────────────────────────────────────────────────

/// Additional log₁₀ copies/L injected at a specific site / pathogen / day.
///
/// Models a Gaussian pulse: amplitude · exp(-(day - peak)² / (2·σ²))
struct OutbreakPulse {
    site_id:    &'static str,
    pathogen:   &'static str,
    peak_day:   f64,
    amplitude:  f64,   // max additional log₁₀ copies/L
    sigma_days: f64,   // pulse width
}

const OUTBREAKS: &[OutbreakPulse] = &[
    // SARS-CoV-2 cluster centred on Belize City South, peaking day 9
    OutbreakPulse { site_id: "bcity_south", pathogen: "SARS-CoV-2", peak_day: 9.0, amplitude: 2.8, sigma_days: 2.0 },
    // Partial spread to north side through shared trunk sewer
    OutbreakPulse { site_id: "bcity_north", pathogen: "SARS-CoV-2", peak_day: 10.0, amplitude: 1.6, sigma_days: 2.0 },
    // WWTP integrates both
    OutbreakPulse { site_id: "bcity_wwtp",  pathogen: "SARS-CoV-2", peak_day: 10.5, amplitude: 1.2, sigma_days: 2.0 },
    // Mpox traveller import — Belmopan, sharper pulse
    OutbreakPulse { site_id: "belmopan_core", pathogen: "Mpox", peak_day: 11.0, amplitude: 3.2, sigma_days: 1.5 },
];

fn outbreak_contribution(site_id: &str, pathogen: &str, day: usize) -> f64 {
    let d = day as f64;
    OUTBREAKS.iter()
        .filter(|o| o.site_id == site_id && o.pathogen == pathogen)
        .map(|o| {
            let delta = d - o.peak_day;
            o.amplitude * (-delta * delta / (2.0 * o.sigma_days * o.sigma_days)).exp()
        })
        .sum()
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Run the Belize simulation and return the populated network model.
///
/// All domain instances are written into `engine`; every significant event is
/// appended to `audit`; anomaly events are processed by `detector`.
pub fn run_belize(
    engine:   &OntologyEngine,
    audit:    &AuditLog,
    detector: &mut AnomalyDetector,
) -> DomainResult<SewageNetwork> {
    // ── Register monitoring sites ─────────────────────────────────────────────
    for site in SITES {
        DomainFactory::create_site(engine, SiteParams {
            site_id:       Some(site.id.to_string()),
            name:          site.name.to_string(),
            region:        site.region.to_string(),
            catchment_pop: site.catchment_pop,
            lat:           site.lat,
            lon:           site.lon,
        })?;
        audit.record("system", AuditAction::SiteAdded, site.id, site.name);
    }

    // ── Register flow links ───────────────────────────────────────────────────
    let flow_links: &[(&str, &str)] = &[
        ("bcity_south",   "bcity_wwtp"),
        ("bcity_north",   "bcity_wwtp"),
        ("belmopan_core", "bcity_wwtp"),
        ("belmopan_ind",  "bcity_wwtp"),
        ("san_ignacio",   "bcity_wwtp"),
        ("orange_walk",   "bcity_north"),
    ];
    for &(up, down) in flow_links {
        DomainFactory::add_flow_link(engine, up, down)?;
    }

    // ── Build spectral network model ──────────────────────────────────────────
    let catchments: Vec<(String, Vec<String>)> = vec![
        ("Belize City Trunk".into(),    vec!["bcity_north".into(), "bcity_south".into(), "bcity_wwtp".into()]),
        ("Belmopan Zone".into(),        vec!["belmopan_core".into(), "belmopan_ind".into()]),
        ("Northern District".into(),    vec!["orange_walk".into()]),
        ("Western Highlands".into(),    vec!["san_ignacio".into()]),
        ("Southern Coastal".into(),     vec!["dangriga".into()]),
    ];
    let extra: Vec<String> = SITES.iter().map(|s| s.id.to_string()).collect();

    let network = SewageNetwork::build(&catchments, &extra)
        .map_err(|e| ww_domain::DomainError::Conversion(e.to_string()))?;

    println!(
        "  Network: {} sites, Fiedler λ₂={:.4} (algebraic connectivity)\n",
        network.n_sites(),
        network.fiedler_value()
    );

    // ── Simulation loop ───────────────────────────────────────────────────────
    const N_DAYS: usize = 15;

    for day in 0..N_DAYS {
        let mut day_alerts = 0usize;

        for path in PATHOGENS {
            // Phase 1: generate all site concentrations for this (day, pathogen)
            let mut concs: HashMap<String, f64>   = HashMap::new();
            let mut sample_map: HashMap<String, String> = HashMap::new();
            let mut signal_map: HashMap<String, String> = HashMap::new();

            for (si, site) in SITES.iter().enumerate() {
                let seed = (day as u64)
                    .wrapping_mul(97)
                    .wrapping_add(si as u64)
                    .wrapping_add(path.name.len() as u64 * 1_000_003);
                let noise    = path.noise_std * approx_normal(seed);
                let outbreak = outbreak_contribution(site.id, path.name, day);
                let conc     = (path.baseline_log + noise + outbreak).max(0.3);

                let sample_id = DomainFactory::create_sample(engine, SampleParams {
                    sample_id:   None,
                    site_id:     site.id.to_string(),
                    flow_liters: 25_000.0 + 5_000.0 * (si as f64),
                    qc_passed:   true,
                    notes:       format!("day {day}"),
                })?;
                let signal_id = DomainFactory::create_signal(engine, SignalParams {
                    signal_id:          None,
                    sample_id:          sample_id.clone(),
                    pathogen:           path.name.to_string(),
                    target_gene:        path.gene.to_string(),
                    log10_copies_per_l: conc,
                    method:             path.method.to_string(),
                })?;

                audit.record(
                    "system",
                    AuditAction::SampleIngested,
                    &sample_id,
                    format!("site={} path={} log10={:.2}", site.id, path.name, conc),
                );

                concs.insert(site.id.to_string(), conc);
                sample_map.insert(site.id.to_string(), sample_id);
                signal_map.insert(site.id.to_string(), signal_id);
            }

            // Phase 2: spectral score for this pathogen's network field
            let spectral = network.spectral_score(&concs);

            // Phase 3: statistical anomaly detection per site
            for site in SITES {
                let conc      = *concs.get(site.id).unwrap();
                let signal_id = signal_map.get(site.id).unwrap();

                if let Some(event) = detector.observe(site.id, path.name, conc, spectral) {
                    let alert_id = DomainFactory::create_alert(engine, AlertParams {
                        alert_id:       None,
                        site_id:        site.id.to_string(),
                        pathogen:       path.name.to_string(),
                        severity:       event.severity.as_str().to_string(),
                        signal_id:      signal_id.clone(),
                        z_score:        event.z_score,
                        spectral_score: event.spectral_score,
                    })?;

                    audit.record(
                        "system",
                        AuditAction::AlertRaised,
                        &alert_id,
                        format!(
                            "site={} path={} z={:.2} spectral={:.3} sev={}",
                            site.id, path.name, event.z_score,
                            event.spectral_score,
                            event.severity.as_str()
                        ),
                    );

                    println!(
                        "  {} [Day {:2}] {:8} ALERT  {}  @  {}\n           \
                         z={:.2}  spectral={:.3}  conc={:.2} log₁₀ copies/L  \
                         n_obs={}\n           alert_id={}",
                        event.severity.as_str().chars().next().unwrap_or('?'),
                        day,
                        event.severity.as_str(),
                        path.name,
                        site.name,
                        event.z_score,
                        event.spectral_score,
                        conc,
                        event.n_obs,
                        alert_id,
                    );
                    day_alerts += 1;
                }
            }
        }

        let arrow = if day_alerts > 0 {
            format!(" ← {} alert(s) raised", day_alerts)
        } else if day < 8 {
            "   [baseline warm-up]".into()
        } else {
            String::new()
        };

        println!("  Day {:2} | {} samples{}", day, SITES.len() * PATHOGENS.len(), arrow);
    }

    Ok(network)
}
