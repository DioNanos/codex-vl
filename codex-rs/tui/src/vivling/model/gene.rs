use serde::Deserialize;
use serde::Serialize;

use super::WorkAffinitySet;
use super::WorkArchetype;
use super::text_utils::fnv1a64;

const GENE_FACTOR_MIN: f32 = 0.70;
const GENE_FACTOR_MAX: f32 = 1.30;
const BRAIN_POTENTIAL_MIN: f32 = 0.80;
const BRAIN_POTENTIAL_MAX: f32 = 1.20;
const TEMPERAMENT_MIN: u8 = 10;
const TEMPERAMENT_MAX: u8 = 90;
const DEFAULT_TEMPERAMENT: u8 = 50;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct VivlingGeneVector {
    #[serde(default = "default_affinity_mod")]
    pub(crate) affinity_mod: [f32; 4],
    #[serde(default = "default_temperament")]
    pub(crate) curiosity: u8,
    #[serde(default = "default_temperament")]
    pub(crate) caution: u8,
    #[serde(default = "default_temperament")]
    pub(crate) sociability: u8,
    #[serde(default = "default_temperament")]
    pub(crate) patience: u8,
    #[serde(default = "default_brain_potential")]
    pub(crate) brain_potential: f32,
}

impl Default for VivlingGeneVector {
    fn default() -> Self {
        Self {
            affinity_mod: default_affinity_mod(),
            curiosity: default_temperament(),
            caution: default_temperament(),
            sociability: default_temperament(),
            patience: default_temperament(),
            brain_potential: default_brain_potential(),
        }
    }
}

impl VivlingGeneVector {
    pub(crate) fn generate(seed: &str) -> Self {
        if seed.trim().is_empty() {
            return Self::default();
        }
        let mut rng = GeneRng::new(fnv1a64(seed.as_bytes()));
        Self {
            affinity_mod: [
                rng.factor(GENE_FACTOR_MIN, GENE_FACTOR_MAX),
                rng.factor(GENE_FACTOR_MIN, GENE_FACTOR_MAX),
                rng.factor(GENE_FACTOR_MIN, GENE_FACTOR_MAX),
                rng.factor(GENE_FACTOR_MIN, GENE_FACTOR_MAX),
            ],
            curiosity: rng.temperament(),
            caution: rng.temperament(),
            sociability: rng.temperament(),
            patience: rng.temperament(),
            brain_potential: rng.factor(BRAIN_POTENTIAL_MIN, BRAIN_POTENTIAL_MAX),
        }
    }

    pub(crate) fn inherit_from(parent: &Self, seed: &str) -> Self {
        if seed.trim().is_empty() {
            return Self::default();
        }
        let mut rng = GeneRng::new(fnv1a64(seed.as_bytes()));
        let mut child = Self::generate(seed);
        for index in strongest_affinity_indices(parent).into_iter().take(2) {
            child.affinity_mod[index] =
                clamp_factor(parent.affinity_mod[index] + rng.small_delta(0.05));
        }
        child.curiosity = mutated_temperament(parent.curiosity, rng.temperament_delta());
        child.caution = mutated_temperament(parent.caution, rng.temperament_delta());
        child.sociability = mutated_temperament(parent.sociability, rng.temperament_delta());
        child.patience = mutated_temperament(parent.patience, rng.temperament_delta());
        child.brain_potential =
            clamp_brain_potential(parent.brain_potential + rng.small_delta(0.05));
        child
    }

    pub(crate) fn is_neutral(&self) -> bool {
        self == &Self::default()
    }

    pub(crate) fn gene_stripe(&self) -> String {
        let parts = [
            ("BLD", self.affinity_mod[0]),
            ("REV", self.affinity_mod[1]),
            ("RSH", self.affinity_mod[2]),
            ("OPS", self.affinity_mod[3]),
        ];
        parts
            .into_iter()
            .map(|(label, value)| {
                format!("{label} {:+03}%", ((value - 1.0) * 100.0).round() as i32)
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    pub(crate) fn temperament_summary(&self) -> String {
        let mut traits = [
            ("curious", self.curiosity),
            ("cautious", self.caution),
            ("social", self.sociability),
            ("patient", self.patience),
        ];
        traits.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        traits
            .into_iter()
            .take(3)
            .map(|(label, _)| label)
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(crate) fn brain_potential_label(&self) -> &'static str {
        if self.brain_potential >= 1.10 {
            "high"
        } else if self.brain_potential <= 0.90 {
            "quiet"
        } else {
            "steady"
        }
    }
}

pub(crate) fn modulated_totals(
    learned: &WorkAffinitySet,
    species_bias: &WorkAffinitySet,
    genes: &VivlingGeneVector,
) -> [(WorkArchetype, u64); 4] {
    [
        (
            WorkArchetype::Builder,
            learned
                .builder
                .saturating_add(modulated(species_bias.builder, genes.affinity_mod[0])),
        ),
        (
            WorkArchetype::Reviewer,
            learned
                .reviewer
                .saturating_add(modulated(species_bias.reviewer, genes.affinity_mod[1])),
        ),
        (
            WorkArchetype::Researcher,
            learned
                .researcher
                .saturating_add(modulated(species_bias.researcher, genes.affinity_mod[2])),
        ),
        (
            WorkArchetype::Operator,
            learned
                .operator
                .saturating_add(modulated(species_bias.operator, genes.affinity_mod[3])),
        ),
    ]
}

pub(crate) fn dominant_with_genes(
    learned: &WorkAffinitySet,
    species_bias: &WorkAffinitySet,
    genes: &VivlingGeneVector,
) -> WorkArchetype {
    modulated_totals(learned, species_bias, genes)
        .into_iter()
        .max_by_key(|(_, value)| *value)
        .map(|(kind, _)| kind)
        .unwrap_or(WorkArchetype::Builder)
}

fn default_affinity_mod() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn default_temperament() -> u8 {
    DEFAULT_TEMPERAMENT
}

fn default_brain_potential() -> f32 {
    1.0
}

fn modulated(value: u64, factor: f32) -> u64 {
    ((value as f32) * factor.clamp(GENE_FACTOR_MIN, GENE_FACTOR_MAX)).round() as u64
}

fn clamp_factor(value: f32) -> f32 {
    value.clamp(GENE_FACTOR_MIN, GENE_FACTOR_MAX)
}

fn clamp_brain_potential(value: f32) -> f32 {
    value.clamp(BRAIN_POTENTIAL_MIN, BRAIN_POTENTIAL_MAX)
}

fn mutated_temperament(value: u8, delta: i16) -> u8 {
    (i16::from(value) + delta).clamp(i16::from(TEMPERAMENT_MIN), i16::from(TEMPERAMENT_MAX)) as u8
}

fn strongest_affinity_indices(parent: &VivlingGeneVector) -> [usize; 4] {
    let mut indexed = [
        (0usize, parent.affinity_mod[0]),
        (1usize, parent.affinity_mod[1]),
        (2usize, parent.affinity_mod[2]),
        (3usize, parent.affinity_mod[3]),
    ];
    indexed.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    [indexed[0].0, indexed[1].0, indexed[2].0, indexed[3].0]
}

struct GeneRng {
    state: u64,
}

impl GeneRng {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x.max(1);
        (x >> 32) as u32
    }

    fn unit(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }

    fn factor(&mut self, min: f32, max: f32) -> f32 {
        let smoothed = (self.unit() + self.unit() + self.unit()) / 3.0;
        min + (max - min) * smoothed
    }

    fn small_delta(&mut self, span: f32) -> f32 {
        (self.unit() * (span * 2.0)) - span
    }

    fn temperament(&mut self) -> u8 {
        (f32::from(TEMPERAMENT_MIN) + (f32::from(TEMPERAMENT_MAX - TEMPERAMENT_MIN) * self.unit()))
            .round() as u8
    }

    fn temperament_delta(&mut self) -> i16 {
        ((self.unit() * 20.0).round() as i16) - 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_gene_vector_is_deterministic_and_clamped() {
        let a = VivlingGeneVector::generate("seed-a");
        let b = VivlingGeneVector::generate("seed-a");
        assert_eq!(a, b);
        for value in a.affinity_mod {
            assert!((GENE_FACTOR_MIN..=GENE_FACTOR_MAX).contains(&value));
        }
        assert!((TEMPERAMENT_MIN..=TEMPERAMENT_MAX).contains(&a.curiosity));
        assert!((TEMPERAMENT_MIN..=TEMPERAMENT_MAX).contains(&a.caution));
        assert!((TEMPERAMENT_MIN..=TEMPERAMENT_MAX).contains(&a.sociability));
        assert!((TEMPERAMENT_MIN..=TEMPERAMENT_MAX).contains(&a.patience));
        assert!((BRAIN_POTENTIAL_MIN..=BRAIN_POTENTIAL_MAX).contains(&a.brain_potential));
    }

    #[test]
    fn inheritance_preserves_two_strongest_affinities() {
        let parent = VivlingGeneVector {
            affinity_mod: [0.80, 1.25, 0.95, 1.18],
            curiosity: 70,
            caution: 30,
            sociability: 55,
            patience: 80,
            brain_potential: 1.12,
        };
        let child = VivlingGeneVector::inherit_from(&parent, "child-seed");
        assert!((child.affinity_mod[1] - parent.affinity_mod[1]).abs() <= 0.05);
        assert!((child.affinity_mod[3] - parent.affinity_mod[3]).abs() <= 0.05);
        assert_ne!(child, parent);
    }

    #[test]
    fn genes_can_change_dominant_archetype_without_mutating_memory() {
        let learned = WorkAffinitySet::default();
        let bias = WorkAffinitySet {
            builder: 100,
            reviewer: 95,
            researcher: 0,
            operator: 0,
        };
        let genes = VivlingGeneVector {
            affinity_mod: [0.70, 1.30, 1.0, 1.0],
            ..VivlingGeneVector::default()
        };
        assert_eq!(
            dominant_with_genes(&learned, &bias, &genes),
            WorkArchetype::Reviewer
        );
        assert_eq!(learned, WorkAffinitySet::default());
    }
}
