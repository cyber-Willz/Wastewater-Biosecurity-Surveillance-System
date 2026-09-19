# ww_biosec — Wastewater Biosecurity Surveillance System

A Rust workspace implementing an end-to-end **wastewater epidemiological
surveillance** pipeline for early outbreak detection.  Wastewater monitoring
can detect pathogen signals **3–7 days before clinical symptoms** appear in a
population, providing critical lead-time for a public-health response.

---

## Architecture

```
ww_biosec/
├── vendor/
│   ├── ontology_engine/        # in-memory knowledge graph (Object/Link model)
│   └── spectral_hypergraph/    # hypergraph Laplacian + spectral analysis
├── ww_domain/                  # Domain schema, typed factory + query layer
├── ww_detection/               # EWMA baseline + spectral network anomaly detection
├── ww_audit/                   # Append-only audit log + analyst review workflow
└── ww_runner/                  # Simulation binary (ww_biosec)
```

### `ww_domain`

Wraps `OntologyEngine` with a typed domain schema:

| Object type        | Primary key  | Description |
|--------------------|-------------|-------------|
| `MonitoringSite`   | `site_id`   | A physical wastewater sampling point |
| `WastewaterSample` | `sample_id` | A collected grab/composite sample |
| `PathogenSignal`   | `signal_id` | log₁₀ copies/L for one pathogen in one sample |
| `SurveillanceAlert`| `alert_id`  | Machine-generated biosurveillance alert |
| `AuditRecord`      | `audit_id`  | Immutable audit trail entry |

Link types encode the provenance chain:
```
SurveillanceAlert –[alert_from_signal]–► PathogenSignal
PathogenSignal    –[signal_from_sample]–► WastewaterSample
WastewaterSample  –[sample_at_site]–► MonitoringSite
MonitoringSite    –[site_flows_to]–► MonitoringSite  (sewer topology)
```

### `ww_detection`

Two complementary detection engines work in tandem:

**Statistical detection** (`EwmaBaseline` + `AnomalyDetector`):

- Per-`(site, pathogen)` EWMA baseline with Welford online variance.
- Anomaly fires when z-score > threshold (default 2.5σ).
- Severity: AMBER (2.5σ) → RED (3.5σ) → CRITICAL (5.0σ).

**Spectral network detection** (`SewageNetwork`):

The sewage network is modelled as a `SpectralHypergraph` where:
- **Vertices** = monitoring sites
- **Hyperedges** = catchment zones

The normalized hypergraph Laplacian Δ encodes expected spatial covariance.  
Two scores quantify network-level anomalies:

```
Rayleigh quotient (norm.)  R(u)/2 = uᵀΔu / (uᵀu · 2)  ∈ [0, 1]
High-freq energy fraction  HFF = 1 − Σ_{k<2}(vₖᵀu)²/uᵀu  ∈ [0, 1]
Composite score            0.6·RQ + 0.4·HFF              ∈ [0, 1]
```

A composite score > 0.40 indicates network-wide spatial inconsistency
(concentrations are too heterogeneous to be explained by normal diffusion)
and upgrades the statistical severity tier by one level.

**Severity matrix:**

| z-score | spectral score | Severity  |
|---------|---------------|-----------|
| ≥ 5.0   | any           | CRITICAL  |
| ≥ 3.5   | any           | RED       |
| ≥ 3.5   | > 0.40        | CRITICAL  |
| ≥ 2.5   | any           | AMBER     |
| ≥ 2.5   | > 0.40        | RED       |
| < 2.5   | any           | (no alert)|

### `ww_audit`

Human-in-the-loop capabilities:

- **`AuditLog`** — thread-safe, append-only event log.  Every ingestion,
  detection, and review decision is recorded.  Export as NDJSON for
  regulatory submission via `audit.export_ndjson()`.
- **`ReviewSession`** — analyst confirms / dismisses / escalates open alerts,
  writing decisions back to the ontology and the audit log atomically.
- **`ReportGenerator`** — builds full `EvidenceChain`s (provenance from alert
  → signal → sample → site → audit trail) and `DailySummary` aggregates.

---

## Belize Simulation Scenario

The `ww_biosec` binary runs a 15-day simulation of 8 Belizean monitoring sites
across Belize City, Belmopan, Orange Walk, San Ignacio, and Dangriga.

Three pathogens are monitored: SARS-CoV-2 (N1 gene, qPCR), Mpox (E6L gene,
ddPCR), and *Vibrio cholerae* (ctxA gene, qPCR).

Simulated outbreak events:
- **Days 8–12**: SARS-CoV-2 cluster at Belize City South, spreading through
  the shared trunk sewer to North Side and the WWTP inlet.
- **Days 11–13**: Mpox traveller import at Belmopan Urban Core.

The system detects both events before simulated clinical presentation, triggers
severity-graded alerts, and walks through an analyst review session.

---

## Building

```bash
# Prerequisites: Rust 1.75+ (rustup recommended)
cargo build --release -p ww_runner

# Run the simulation
cargo run --release --bin ww_biosec
```

---

## Extending

| Task | Where |
|------|-------|
| Add a new pathogen | `ww_runner/src/scenario.rs` → `PATHOGENS` |
| Add a monitoring site | `scenario.rs` → `SITES` + update `catchments` |
| Change alert thresholds | `ww_detection/src/detector.rs` → `DEFAULT_Z_THRESHOLD` |
| Plug in real LIMS data | Replace `DomainFactory::create_sample/signal` calls |
| Connect to a PostgreSQL store | Swap `OntologyEngine` for a persistent backend |
| REST API | Add `axum` router in a new `ww_api` crate |

---

## Regulatory alignment

- Alert evidence chains are exportable per WHO IHR Article 6 notification
  requirements.
- The audit log is append-only and timestamped to ISO-8601 UTC, suitable for
  laboratory information management system (LIMS) integration.
- The NDJSON export format is compatible with ECDC's EWS data exchange schema.
