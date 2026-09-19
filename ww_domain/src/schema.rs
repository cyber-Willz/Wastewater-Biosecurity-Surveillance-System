//! Domain-wide ontology schema.
//!
//! Object types and link types are registered once at startup via
//! [`register_biosec_schema`].  All name constants are exported so other
//! crates can address the schema without hard-coding strings.

use ontology_engine::prelude::*;
use crate::Result;

// ── Object-type name constants ───────────────────────────────────────────────

/// A physical wastewater sampling point in the sewage network.
pub const MONITORING_SITE: &str = "MonitoringSite";
/// A grab- or composite-sample collected at a monitoring site.
pub const WASTEWATER_SAMPLE: &str = "WastewaterSample";
/// A single pathogen or AMR-marker signal detected in a sample.
pub const PATHOGEN_SIGNAL: &str = "PathogenSignal";
/// A machine-generated or analyst-escalated biosurveillance alert.
pub const SURVEILLANCE_ALERT: &str = "SurveillanceAlert";
/// An immutable audit record written by any system actor.
pub const AUDIT_RECORD: &str = "AuditRecord";

// ── Link-type name constants ─────────────────────────────────────────────────

/// WastewaterSample → MonitoringSite  (where the sample was collected).
pub const SAMPLE_AT_SITE: &str = "sample_at_site";
/// PathogenSignal → WastewaterSample  (which sample the signal came from).
pub const SIGNAL_FROM_SAMPLE: &str = "signal_from_sample";
/// SurveillanceAlert → PathogenSignal  (which signal triggered the alert).
pub const ALERT_FROM_SIGNAL: &str = "alert_from_signal";
/// MonitoringSite → MonitoringSite  (upstream site drains into downstream site).
pub const SITE_FLOWS_TO: &str = "site_flows_to";

// ── Schema registration ──────────────────────────────────────────────────────

/// Register every object type and link type required by the biosurveillance
/// system.  Must be called once before any factory or query function is used.
pub fn register_biosec_schema(engine: &OntologyEngine) -> Result<()> {
    // ── MonitoringSite ───────────────────────────────────────────────────────
    // Primary key: site_id (String)
    // Properties : name, region, catchment_pop, lat, lon, active
    let site_type = ObjectTypeBuilder::new(MONITORING_SITE)
        .primary_key("site_id")
        .property("site_id",       PropertyType::String)
        .property("name",          PropertyType::String)
        .property("region",        PropertyType::String)
        .property("catchment_pop", PropertyType::Integer)
        .property("lat",           PropertyType::Float)
        .property("lon",           PropertyType::Float)
        .property("active",        PropertyType::Boolean)
        .build()
        .map_err(|e| ontology_engine::error::OntologyError::EmptyPrimaryKey { name: e })?;

    // ── WastewaterSample ─────────────────────────────────────────────────────
    // Primary key: sample_id (String)
    // Properties : site_id (FK ref), collected_at (ISO-8601), flow_liters,
    //              qc_passed, notes
    let sample_type = ObjectTypeBuilder::new(WASTEWATER_SAMPLE)
        .primary_key("sample_id")
        .property("sample_id",    PropertyType::String)
        .property("site_id",      PropertyType::String)
        .property("collected_at", PropertyType::String)
        .property("flow_liters",  PropertyType::Float)
        .property("qc_passed",    PropertyType::Boolean)
        .property("notes",        PropertyType::String)
        .build()
        .map_err(|e| ontology_engine::error::OntologyError::EmptyPrimaryKey { name: e })?;

    // ── PathogenSignal ───────────────────────────────────────────────────────
    // Primary key: signal_id (String)
    // Properties : sample_id (FK ref), pathogen, target_gene,
    //              log10_copies_per_l, method
    let signal_type = ObjectTypeBuilder::new(PATHOGEN_SIGNAL)
        .primary_key("signal_id")
        .property("signal_id",          PropertyType::String)
        .property("sample_id",          PropertyType::String)
        .property("pathogen",           PropertyType::String)
        .property("target_gene",        PropertyType::String)
        .property("log10_copies_per_l", PropertyType::Float)
        .property("method",             PropertyType::String)
        .build()
        .map_err(|e| ontology_engine::error::OntologyError::EmptyPrimaryKey { name: e })?;

    // ── SurveillanceAlert ────────────────────────────────────────────────────
    // Primary key: alert_id (String)
    // Properties : site_id, pathogen, severity, signal_id (FK ref),
    //              detected_at, status, z_score, spectral_score, analyst_notes
    let alert_type = ObjectTypeBuilder::new(SURVEILLANCE_ALERT)
        .primary_key("alert_id")
        .property("alert_id",       PropertyType::String)
        .property("site_id",        PropertyType::String)
        .property("pathogen",       PropertyType::String)
        .property("severity",       PropertyType::String)
        .property("signal_id",      PropertyType::String)
        .property("detected_at",    PropertyType::String)
        .property("status",         PropertyType::String)
        .property("z_score",        PropertyType::Float)
        .property("spectral_score", PropertyType::Float)
        .property("analyst_notes",  PropertyType::String)
        .build()
        .map_err(|e| ontology_engine::error::OntologyError::EmptyPrimaryKey { name: e })?;

    // ── AuditRecord ──────────────────────────────────────────────────────────
    // Primary key: audit_id (String)
    // Properties : timestamp, actor, action, target_id, details
    let audit_type = ObjectTypeBuilder::new(AUDIT_RECORD)
        .primary_key("audit_id")
        .property("audit_id",  PropertyType::String)
        .property("timestamp", PropertyType::String)
        .property("actor",     PropertyType::String)
        .property("action",    PropertyType::String)
        .property("target_id", PropertyType::String)
        .property("details",   PropertyType::String)
        .build()
        .map_err(|e| ontology_engine::error::OntologyError::EmptyPrimaryKey { name: e })?;

    engine.register_object_type(site_type)?;
    engine.register_object_type(sample_type)?;
    engine.register_object_type(signal_type)?;
    engine.register_object_type(alert_type)?;
    engine.register_object_type(audit_type)?;

    // ── Link types ───────────────────────────────────────────────────────────
    engine.register_link_type(LinkType::new(
        SAMPLE_AT_SITE,
        WASTEWATER_SAMPLE,
        MONITORING_SITE,
    ))?;
    engine.register_link_type(LinkType::new(
        SIGNAL_FROM_SAMPLE,
        PATHOGEN_SIGNAL,
        WASTEWATER_SAMPLE,
    ))?;
    engine.register_link_type(LinkType::new(
        ALERT_FROM_SIGNAL,
        SURVEILLANCE_ALERT,
        PATHOGEN_SIGNAL,
    ))?;
    engine.register_link_type(LinkType::new(
        SITE_FLOWS_TO,
        MONITORING_SITE,
        MONITORING_SITE,
    ))?;

    Ok(())
}
