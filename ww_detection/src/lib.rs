//! `ww_detection` — Signal detection for wastewater biosurveillance.
//!
//! Two complementary detection strategies are combined:
//!
//! 1. **Statistical** (`baseline`, `detector`): per-`(site, pathogen)` EWMA
//!    baseline with Welford online variance.  An anomaly fires when the
//!    z-score of an incoming concentration measurement exceeds a configurable
//!    threshold.
//!
//! 2. **Spectral** (`spectral`): the sewage network is modelled as a
//!    [`spectral_hypergraph::SpectralHypergraph`] where monitoring sites are
//!    vertices and catchment zones are hyperedges.  The normalized hypergraph
//!    Laplacian Δ encodes the expected spatial smoothness of any normally-
//!    distributed pathogen field.  Two scores are computed per observation
//!    round:
//!
//!    * **Rayleigh quotient** `R(u) = uᵀΔu / uᵀu` — high value means the
//!      concentration field is spatially rough (concentrations at adjacent
//!      sites diverge more than diffusion would predict).
//!    * **High-frequency energy fraction** — fraction of the field's L² energy
//!      that lies in Laplacian eigenvectors beyond the `k` smoothest modes.
//!
//!    The composite spectral score upgrades the statistical severity tier when
//!    an anomaly is simultaneously local (high z-score) *and* network-wide
//!    (high spectral score), which is the earliest detectable signature of a
//!    spreading outbreak.

pub mod baseline;
pub mod detector;
pub mod spectral;

pub use baseline::EwmaBaseline;
pub use detector::{AnomalyDetector, AnomalyEvent, DetectionSeverity};
pub use spectral::SewageNetwork;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DetectionError {
    #[error("spectral hypergraph error: {0}")]
    Hypergraph(#[from] spectral_hypergraph::HypergraphError),

    #[error("baseline has only {n} observations for site '{site}' / pathogen '{pathogen}' (need {need})")]
    InsufficientHistory {
        site:    String,
        pathogen: String,
        n:       usize,
        need:    usize,
    },
}

pub type Result<T> = std::result::Result<T, DetectionError>;
