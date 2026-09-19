//! `ww_biosec` — Wastewater Biosecurity Surveillance System
//!
//! Entry point for the Belize national wastewater-epidemiology simulation.
//! Demonstrates the full pipeline:
//!
//! 1. Schema registration (`ww_domain`)
//! 2. Site and flow-network setup
//! 3. 15-day simulation with EWMA + spectral anomaly detection (`ww_detection`)
//! 4. Analyst review session (`ww_audit`)
//! 5. Evidence-chain reporting and summary

mod scenario;

use ontology_engine::prelude::OntologyEngine;

use ww_audit::{AuditAction, AuditLog, ReportGenerator, ReviewOutcome, ReviewSession};
use ww_detection::AnomalyDetector;
use ww_domain::{query::list_open_alerts, register_biosec_schema};

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║   WASTEWATER BIOSECURITY SURVEILLANCE SYSTEM  v0.1           ║");
    println!("║   Island Carib Real Estate                                   ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ── 1. Initialise shared state ────────────────────────────────────────────
    let engine = OntologyEngine::new();
    let audit  = AuditLog::new();

    // ── 2. Register domain schema ─────────────────────────────────────────────
    print!("▶ Registering biosurveillance schema... ");
    register_biosec_schema(&engine).expect("schema registration failed");
    audit.record("system", AuditAction::SchemaRegistered, "ww_biosec",
        "5 object types, 4 link types");
    println!("done.\n");

    // ── 3. Build detector and run simulation ──────────────────────────────────
    let mut detector = AnomalyDetector::new();

    println!("▶ Building Belize sewage network and running 15-day simulation...\n");
    let _network = scenario::run_belize(&engine, &audit, &mut detector)
        .expect("simulation failed");

    // ── 4. Analyst review session ─────────────────────────────────────────────
    println!("\n══════════════════════ ANALYST REVIEW ══════════════════════\n");
    let session  = ReviewSession::new(&engine, &audit, "Dr. E. Martinez (BPHS)");
    let pending  = session.pending_alerts();
    println!("  {} alert(s) pending review (critical-first order)\n", pending.len());

    for (i, alert) in pending.iter().enumerate() {
        println!(
            "  [{:>2}] {}  {:<10}  {:<25}  site={}",
            i + 1,
            alert.severity.emoji(),
            alert.severity.as_str(),
            alert.pathogen,
            alert.site_id,
        );
    }
    println!();

    // Apply analyst decisions
    for alert in &pending {
        let outcome = match (alert.pathogen.as_str(), alert.site_id.as_str()) {
            ("SARS-CoV-2", "bcity_south") => ReviewOutcome::Confirm {
                notes: "Clinically corroborated: ER triage up 18 % week-on-week. \
                        Coordinating MOH rapid-response.".into(),
            },
            ("SARS-CoV-2", "bcity_north") | ("SARS-CoV-2", "bcity_wwtp") =>
                ReviewOutcome::Confirm {
                    notes: "Part of confirmed Belize City SARS-CoV-2 cluster.".into(),
                },
            ("Mpox", _) => ReviewOutcome::Escalate {
                to_team: "CARPHA / PAHO Regional Lab".into(),
                notes:   "Requesting confirmatory WGS sequencing. \
                          Port-of-entry contact tracing initiated.".into(),
            },
            _ => ReviewOutcome::Dismiss {
                reason: "Within 2-σ on clinical syndromic data; likely baseline \
                         fluctuation.".into(),
            },
        };

        let outcome_label = match &outcome {
            ReviewOutcome::Confirm  { .. }    => "CONFIRMED",
            ReviewOutcome::Dismiss  { .. }    => "DISMISSED",
            ReviewOutcome::Escalate { .. }    => "ESCALATED",
        };

        session.review(&alert.alert_id, outcome)
            .expect("review failed");

        println!(
            "  {:>10}  {}  {} ({})",
            outcome_label,
            alert.severity.emoji(),
            alert.alert_id,
            alert.pathogen
        );
    }

    // ── 5. Summary report ─────────────────────────────────────────────────────
    println!("\n════════════════════════ DAILY REPORT ══════════════════════════\n");
    let reporter = ReportGenerator::new(&engine, &audit);
    print!("{}", reporter.render_summary());

    // ── 6. Evidence chains for confirmed critical alerts ──────────────────────
    println!("\n══════════════════════ EVIDENCE CHAINS ══════════════════════════\n");
    let _remaining_open = list_open_alerts(&engine);
    let chains_to_show: Vec<_> = {
        // Show escalated alerts first, then confirmed, up to 2 total
        let mut all = ww_domain::query::list_all_alerts(&engine);
        all.sort_by(|a, b| b.severity.cmp(&a.severity));
        all.into_iter().take(2).collect()
    };

    for alert in chains_to_show {
        let chain = reporter.evidence_chain(alert);
        print!("{}", chain.render());
    }

    // ── 7. Audit log tail ─────────────────────────────────────────────────────
    println!("══════════════════════ AUDIT LOG (last 12) ══════════════════════\n");
    for entry in audit.recent(12) {
        println!(
            "  [{}]  {:22}  actor={:<20}  target={}",
            entry.timestamp, entry.action, entry.actor, entry.target_id
        );
    }

    println!("\n  Total audit entries : {}", audit.len());
    println!("  Total instances     : {}", engine.instance_count());
    println!("  Total links         : {}", engine.link_count());
    println!("\n▶ Simulation complete.\n");
}
