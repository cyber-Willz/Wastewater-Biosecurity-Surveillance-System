//! Per-`(site, pathogen)` EWMA baseline with Welford online variance.
//!
//! Wastewater concentrations are log₁₀-transformed before ingestion, which
//! converts the roughly log-normal distribution of viral copies into something
//! closer to Gaussian.  The EWMA tracks the centre of that distribution;
//! Welford's algorithm tracks its spread without storing raw history.
//!
//! ## Why two means?
//!
//! * `ewma` adapts quickly to true seasonal drift (controlled by `alpha`).
//! * `welford_mean` / `welford_m2` track long-run variance independent of
//!   recent level shifts.  The z-score uses `ewma` as the centre but the
//!   Welford variance as the spread, so the detector is sensitive to sudden
//!   spikes while remaining robust against slow baseline drift.

use std::collections::HashMap;

/// EWMA smoothing factor.  0.25 gives a half-life of roughly 2.4 observations,
/// fast enough to track week-to-week trends while damping day-to-day noise.
const ALPHA: f64 = 0.25;
/// Minimum observations before the baseline is trusted for anomaly scoring.
const MIN_OBS: usize = 7;

// ── Internal state per (site, pathogen) ──────────────────────────────────────

#[derive(Debug, Clone)]
struct State {
    ewma:          f64,
    welford_mean:  f64,
    welford_m2:    f64,
    /// Sample variance derived from M2 / (n - 1); cached after each update.
    variance:      f64,
    n:             usize,
}

impl State {
    fn new(first: f64) -> Self {
        Self {
            ewma:         first,
            welford_mean: first,
            welford_m2:   0.0,
            variance:     1e-6,      // small non-zero prior avoids /0 on first z-score
            n:            1,
        }
    }

    fn update(&mut self, obs: f64) {
        self.n += 1;

        // Welford pass
        let delta = obs - self.welford_mean;
        self.welford_mean += delta / self.n as f64;
        let delta2 = obs - self.welford_mean;
        self.welford_m2 += delta * delta2;
        self.variance = if self.n > 1 {
            (self.welford_m2 / (self.n - 1) as f64).max(1e-6)
        } else {
            1e-6
        };

        // EWMA pass
        self.ewma = ALPHA * obs + (1.0 - ALPHA) * self.ewma;
    }

    fn z_score(&self, obs: f64) -> f64 {
        (obs - self.ewma) / self.variance.sqrt().max(1e-4)
    }

    fn is_warm(&self) -> bool {
        self.n >= MIN_OBS
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Thread-local EWMA baseline registry.
///
/// Each entry is keyed by `(site_id, pathogen)` and holds the full running
/// state for that pair.  The registry is not `Send`; wrap in `Arc<Mutex<…>>`
/// if sharing across threads.
#[derive(Debug, Default)]
pub struct EwmaBaseline {
    states: HashMap<(String, String), State>,
}

/// Summary returned by [`EwmaBaseline::ingest`].
#[derive(Debug, Clone)]
pub struct BaselineUpdate {
    /// Current EWMA of log₁₀ concentration.
    pub ewma:     f64,
    /// z-score of the current observation relative to the EWMA + variance.
    /// `0.0` when the baseline is still warming up.
    pub z_score:  f64,
    /// Number of observations seen for this `(site, pathogen)` pair.
    pub n:        usize,
    /// `true` when there are enough observations for reliable z-scores.
    pub is_warm:  bool,
}

impl EwmaBaseline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingest one observation and return the updated baseline summary.
    ///
    /// `log10_copies`: log₁₀(copies / L) after flow normalisation.
    pub fn ingest(&mut self, site: &str, pathogen: &str, log10_copies: f64) -> BaselineUpdate {
        let key = (site.to_string(), pathogen.to_string());
        let state = self.states.entry(key).or_insert_with(|| State::new(log10_copies));

        let z = if state.is_warm() {
            state.z_score(log10_copies)
        } else {
            0.0
        };
        let is_warm = state.is_warm();
        state.update(log10_copies);

        BaselineUpdate {
            ewma:    state.ewma,
            z_score: z,
            n:       state.n,
            is_warm,
        }
    }

    pub fn ewma(&self, site: &str, pathogen: &str) -> Option<f64> {
        self.states
            .get(&(site.to_string(), pathogen.to_string()))
            .map(|s| s.ewma)
    }

    pub fn n_obs(&self, site: &str, pathogen: &str) -> usize {
        self.states
            .get(&(site.to_string(), pathogen.to_string()))
            .map(|s| s.n)
            .unwrap_or(0)
    }
}
