//! `ww_domain` — Domain schema, typed factory helpers, and typed query wrappers
//! for the wastewater biosurveillance ontology.
//!
//! All persistent state lives in an [`ontology_engine::engine::OntologyEngine`]
//! that is shared (behind `Arc`) across every crate.  This layer gives the
//! rest of the system strongly-typed domain structs rather than raw
//! [`ontology_engine::types::ObjectInstance`] maps.

pub mod factory;
pub mod query;
pub mod schema;

pub use factory::{AlertParams, DomainFactory, SampleParams, SignalParams, SiteParams};
pub use query::{
    AlertStatus, MonitoringSite, PathogenSignal, Severity, SurveillanceAlert,
    WastewaterSample,
};
pub use schema::register_biosec_schema;

use thiserror::Error;

/// Unified error type for domain-layer operations.
#[derive(Debug, Error)]
pub enum DomainError {
    #[error("ontology error: {0}")]
    Ontology(#[from] ontology_engine::error::OntologyError),

    #[error("missing property '{0}' on instance '{1}'")]
    MissingProperty(String, String),

    #[error("type conversion error: {0}")]
    Conversion(String),
}

pub type Result<T> = std::result::Result<T, DomainError>;
