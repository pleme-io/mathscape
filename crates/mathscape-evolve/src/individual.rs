use mathscape_core::hash::TermRef;
use mathscape_core::term::Term;

/// A single individual in the population.
#[derive(Clone, Debug)]
pub struct Individual {
    /// The root expression of this individual.
    pub term: Term,
    /// Content hash of the root term (for dedup and storage lookup).
    pub hash: TermRef,
    /// Combined fitness score.
    pub fitness: f64,
    /// Compression contribution component of fitness.
    pub cr_contrib: f64,
    /// Novelty component of fitness.
    pub novelty: f64,
    /// MAP-Elites: expression depth bin.
    pub depth_bin: u32,
    /// MAP-Elites: distinct operator count bin.
    pub op_diversity: u32,
    /// MAP-Elites: compression contribution bin.
    pub cr_bin: u32,
}

impl Individual {
    pub fn new(term: Term) -> Self {
        let hash = term.content_hash();
        let depth_bin = (term.depth().min(20) as u32) / 2; // 10 bins
        let op_diversity = term.distinct_ops().min(10) as u32;
        Individual {
            term,
            hash,
            fitness: 0.0,
            cr_contrib: 0.0,
            novelty: 0.0,
            depth_bin,
            op_diversity,
            cr_bin: 0,
        }
    }

    /// Recompute MAP-Elites bins after mutation.
    pub fn update_bins(&mut self) {
        self.hash = self.term.content_hash();
        self.depth_bin = (self.term.depth().min(20) as u32) / 2;
        self.op_diversity = self.term.distinct_ops().min(10) as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn new_computes_correct_hash_and_bins() {
        // add(1, 2) — depth=2, distinct_ops = {Apply, Var, Number} = 3
        let term = apply(var(2), vec![nat(1), nat(2)]);
        let expected_hash = term.content_hash();
        let expected_depth_bin = (term.depth().min(20) as u32) / 2;
        let expected_op_diversity = term.distinct_ops().min(10) as u32;

        let ind = Individual::new(term.clone());

        assert_eq!(ind.hash, expected_hash, "hash should match content_hash()");
        assert_eq!(
            ind.depth_bin, expected_depth_bin,
            "depth_bin should be depth/2"
        );
        assert_eq!(
            ind.op_diversity, expected_op_diversity,
            "op_diversity should match distinct_ops"
        );
        assert_eq!(ind.fitness, 0.0, "initial fitness should be 0.0");
        assert_eq!(ind.cr_contrib, 0.0);
        assert_eq!(ind.novelty, 0.0);
        assert_eq!(ind.cr_bin, 0);
        assert_eq!(ind.term, term);
    }

    #[test]
    fn update_bins_recomputes_after_term_change() {
        let term1 = nat(1);
        let mut ind = Individual::new(term1);

        let old_hash = ind.hash;
        let old_depth_bin = ind.depth_bin;
        let old_op_diversity = ind.op_diversity;

        // Replace term with a deeper, more complex one
        ind.term = apply(var(2), vec![apply(var(3), vec![nat(5), nat(6)]), nat(7)]);
        ind.update_bins();

        // Hash should change because the term changed
        assert_ne!(ind.hash, old_hash, "hash should change after term change");
        // The new term has depth 3, so depth_bin = 3/2 = 1; old was depth 1, bin = 0
        assert!(
            ind.depth_bin != old_depth_bin || ind.op_diversity != old_op_diversity,
            "at least one bin should change with a structurally different term"
        );
        // Verify bins match the new term
        assert_eq!(ind.hash, ind.term.content_hash());
        assert_eq!(ind.depth_bin, (ind.term.depth().min(20) as u32) / 2);
        assert_eq!(ind.op_diversity, ind.term.distinct_ops().min(10) as u32);
    }
}
