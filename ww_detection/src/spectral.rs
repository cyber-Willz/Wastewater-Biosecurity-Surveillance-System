//! Spectral network anomaly scoring for the wastewater sewage graph.
//!
//! The sewage network is modelled as a [`SpectralHypergraph`]:
//! * **Vertices** — monitoring sites (one vertex per site).
//! * **Hyperedges** — catchment zones (groups of sites that drain into the
//!   same trunk sewer or WWTP inlet), plus one weak background hyperedge
//!   connecting all sites to prevent isolated-vertex degeneracy.
//!
//! The normalized hypergraph Laplacian `Δ` encodes the expected spatial
//! smoothness of any pathogen field.  Two scores quantify deviation:
//!
//! | Score | Formula | Range | Meaning |
//! |---|---|---|---|
//! | Rayleigh quotient (norm.) | `uᵀΔu / (uᵀu · 2)` | `[0,1]` | 0=smooth, 1=rough |
//! | HF energy fraction | `1 − Σ_{k<K}(vₖᵀu)²/uᵀu` | `[0,1]` | fraction in high modes |
//! | Composite | `0.6·RQ + 0.4·HFF` | `[0,1]` | combined network score |

use std::collections::HashMap;

use nalgebra::{DMatrix, DVector};
use spectral_hypergraph::{HypergraphBuilder, VertexId};
use spectral_hypergraph::laplacian::dense_normalized_laplacian;
use spectral_hypergraph::spectral::{dense_eigen, EigenDecomposition};

use crate::Result;

// Weight of the global background hyperedge that keeps every vertex connected.
const BACKGROUND_WEIGHT: f64 = 0.05;

/// A compiled sewage-network spectral model, ready for repeated scoring.
pub struct SewageNetwork {
    /// Site IDs in row-order of the Laplacian.
    pub site_ids: Vec<String>,
    laplacian:    DMatrix<f64>,
    eig:          EigenDecomposition,
}

impl SewageNetwork {
    /// Build the spectral model from catchment definitions.
    ///
    /// `catchments`: `(catchment_label, member_site_ids)` pairs.
    /// `extra_sites`: additional site IDs to include even when they are not in
    /// any named catchment (they will still participate in the background edge).
    ///
    /// Hyperedges with `|members| < 2` are silently skipped.
    pub fn build(
        catchments:  &[(String, Vec<String>)],
        extra_sites: &[String],
    ) -> Result<Self> {
        let mut builder     = HypergraphBuilder::new();
        let mut site_ids    = Vec::<String>::new();
        let mut site_to_vid = HashMap::<String, VertexId>::new();

        // ── Deduplicate site IDs in stable order ─────────────────────────────
        let mut seen = std::collections::HashSet::<String>::new();
        let mut register = |s: &str| {
            if seen.insert(s.to_string()) {
                site_ids.push(s.to_string());
            }
        };
        for (_, members) in catchments {
            for s in members { register(s); }
        }
        for s in extra_sites { register(s); }

        // ── Add vertices ─────────────────────────────────────────────────────
        for s in &site_ids {
            let vid = builder.add_vertex(s.as_str())?;
            site_to_vid.insert(s.clone(), vid);
        }

        // ── Add catchment hyperedges ──────────────────────────────────────────
        for (_, members) in catchments {
            let vids: Vec<VertexId> = members
                .iter()
                .filter_map(|s| site_to_vid.get(s).copied())
                .collect();
            if vids.len() >= 2 {
                builder.add_hyperedge(&vids, 1.0)?;
            }
        }

        // ── Global background edge (prevents IsolatedVertex degeneracy) ───────
        // All sites are members of this single weak hyperedge, so that even
        // sites belonging to no named catchment still have nonzero degree and
        // the normalized Laplacian is well-defined.
        let all_vids: Vec<VertexId> =
            site_ids.iter().filter_map(|s| site_to_vid.get(s).copied()).collect();
        if all_vids.len() >= 2 {
            builder.add_hyperedge(&all_vids, BACKGROUND_WEIGHT)?;
        }

        let hg        = builder.build()?;
        let laplacian = dense_normalized_laplacian(&hg)?;
        let eig       = dense_eigen(&laplacian);

        Ok(Self { site_ids, laplacian, eig })
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn field_vec(&self, concs: &HashMap<String, f64>) -> DVector<f64> {
        DVector::from_vec(
            self.site_ids
                .iter()
                .map(|s| *concs.get(s).unwrap_or(&0.0))
                .collect(),
        )
    }

    // ── Public scores ────────────────────────────────────────────────────────

    /// Normalised Rayleigh quotient `R(u)/2 ∈ [0, 1]`.
    ///
    /// Low value: the pathogen field is spatially smooth — concentrations vary
    /// gradually between connected sites (consistent with normal diffusion).
    /// High value: the field has large high-frequency components — adjacent
    /// sites have sharply diverging concentrations, the hallmark of a
    /// localised outbreak that has not yet diffused through the network.
    pub fn rayleigh_quotient_norm(&self, concs: &HashMap<String, f64>) -> f64 {
        let u     = self.field_vec(concs);
        let norm2 = u.dot(&u);
        if norm2 < 1e-10 { return 0.0; }
        let delta_u = &self.laplacian * &u;
        (u.dot(&delta_u) / norm2 / 2.0).clamp(0.0, 1.0)
    }

    /// Fraction of field energy in Laplacian eigenmodes beyond the `k`
    /// smoothest (`k = 2` keeps the DC component and Fiedler direction).
    pub fn high_freq_fraction(&self, concs: &HashMap<String, f64>, k: usize) -> f64 {
        let u     = self.field_vec(concs);
        let total = u.dot(&u);
        if total < 1e-10 { return 0.0; }
        let k = k.min(self.site_ids.len());
        let low: f64 = (0..k).map(|i| {
            let v = self.eig.eigenvectors.column(i);
            let c = v.dot(&u);
            c * c
        }).sum();
        ((total - low) / total).clamp(0.0, 1.0)
    }

    /// Composite spectral anomaly score in `[0, 1]`.
    ///
    /// Scores > 0.40 indicate network-wide spatial inconsistency and trigger a
    /// one-tier severity upgrade in [`crate::detector::DetectionSeverity`].
    pub fn spectral_score(&self, concs: &HashMap<String, f64>) -> f64 {
        let rq  = self.rayleigh_quotient_norm(concs);
        let hff = self.high_freq_fraction(concs, 2);
        (0.6 * rq + 0.4 * hff).clamp(0.0, 1.0)
    }

    /// Fiedler value (λ₂ of Δ): the algebraic connectivity of the network.
    /// Near-zero → weakly connected network; larger → well-mixed topology.
    pub fn fiedler_value(&self) -> f64 {
        self.eig.eigenvalues.iter().copied()
            .find(|&v| v > 1e-8)
            .unwrap_or(0.0)
    }

    pub fn n_sites(&self) -> usize { self.site_ids.len() }
}
