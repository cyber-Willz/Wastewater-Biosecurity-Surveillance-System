//! Threshold-based anomaly detector combining statistical and spectral scores.
//!
//! ## Severity mapping
//!
//! | z-score  | spectral score | Severity  |
//! |----------|---------------|-----------|
//! | ≥ 5.0    | any           | CRITICAL  |
//! | ≥ 3.5    | any           | RED       |
//! | ≥ 2.5    | any           | AMBER     |
//! | ≥ 2.5    | > 0.40        | RED   (+1 upgrade from network spread)  |
//! | ≥ 3.5    | > 0.40        | CRITICAL  (+1 upgrade) |
//! | < 2.5    | any           | (no alert) |

use crate::baseline::EwmaBaseline;

// Default z-score threshold for raising any alert.
const DEFAULT_Z_THRESHOLD: f64 = 2.5;
// Spectral score above which network-spread upgrade applies.
const NETWORK_SPREAD_THRESHOLD: f64 = 0.40;

/// Mirrors [`ww_domain::query::Severity`] without importing the domain crate.
/// Converted by the runner before writing to the ontology.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DetectionSeverity {
    Green,
    Amber,
    Red,
    Critical,
}

impl DetectionSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            DetectionSeverity::Green    => "GREEN",
            DetectionSeverity::Amber    => "AMBER",
            DetectionSeverity::Red      => "RED",
            DetectionSeverity::Critical => "CRITICAL",
        }
    }

    /// Derive severity from a statistical z-score optionally upgraded by the
    /// spectral network score.
    pub fn from_scores(z: f64, spectral: f64) -> Self {
        let base = if z >= 5.0 {
            DetectionSeverity::Critical
        } else if z >= 3.5 {
            DetectionSeverity::Red
        } else if z >= 2.5 {
            DetectionSeverity::Amber
        } else {
            DetectionSeverity::Green
        };

        // Network-spread upgrade: isolated spike → only statistical; widespread
        // anomalous gradient pattern → one tier upgrade.
        if spectral > NETWORK_SPREAD_THRESHOLD {
            match base {
                DetectionSeverity::Amber => DetectionSeverity::Red,
                DetectionSeverity::Red   => DetectionSeverity::Critical,
                other                    => other,
            }
        } else {
            base
        }
    }
}

/// An anomaly event ready to be lifted into a [`ww_domain`] alert.
#[derive(Debug, Clone)]
pub struct AnomalyEvent {
    pub site_id:        String,
    pub pathogen:       String,
    /// Observed log₁₀(copies/L).
    pub log10_copies:   f64,
    /// EWMA baseline at time of detection.
    pub ewma:           f64,
    /// Statistical z-score.
    pub z_score:        f64,
    /// Composite spectral network anomaly score in `[0, 1]`.
    pub spectral_score: f64,
    pub severity:       DetectionSeverity,
    /// Number of observations in the baseline at time of detection.
    pub n_obs:          usize,
}

/// Stateful anomaly detector.
///
/// Maintains one [`EwmaBaseline`] across all `(site, pathogen)` pairs.
/// Call [`AnomalyDetector::observe`] once per sample-signal; the detector
/// handles baseline warm-up internally.
pub struct AnomalyDetector {
    baseline:    EwmaBaseline,
    z_threshold: f64,
}

impl AnomalyDetector {
    pub fn new() -> Self {
        Self {
            baseline:    EwmaBaseline::new(),
            z_threshold: DEFAULT_Z_THRESHOLD,
        }
    }

    pub fn with_z_threshold(mut self, z: f64) -> Self {
        self.z_threshold = z;
        self
    }

    /// Ingest one signal measurement.
    ///
    /// Returns `Some(event)` if the measurement crosses the detection
    /// threshold, `None` otherwise (including during baseline warm-up).
    ///
    /// `spectral_score`: the pre-computed [`SewageNetwork::spectral_score`]
    /// for the current observation round.
    pub fn observe(
        &mut self,
        site_id:        &str,
        pathogen:       &str,
        log10_copies:   f64,
        spectral_score: f64,
    ) -> Option<AnomalyEvent> {
        let update = self.baseline.ingest(site_id, pathogen, log10_copies);

        if !update.is_warm || update.z_score < self.z_threshold {
            return None;
        }

        let severity = DetectionSeverity::from_scores(update.z_score, spectral_score);
        Some(AnomalyEvent {
            site_id:        site_id.to_string(),
            pathogen:       pathogen.to_string(),
            log10_copies,
            ewma:           update.ewma,
            z_score:        update.z_score,
            spectral_score,
            severity,
            n_obs:          update.n,
        })
    }

    pub fn baseline_mut(&mut self) -> &mut EwmaBaseline {
        &mut self.baseline
    }
}

impl Default for AnomalyDetector {
    fn default() -> Self {
        Self::new()
    }
}
