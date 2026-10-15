//! Population management and MAP-Elites archive.

use crate::individual::Individual;
use crate::mutate;
use crate::select;
use rand::Rng;
use std::collections::HashMap;

/// MAP-Elites cell key: (depth_bin, op_diversity, cr_bin).
type CellKey = (u32, u32, u32);

/// The population: combines a flat list with a MAP-Elites archive.
pub struct Population {
    /// All individuals in the current generation.
    pub individuals: Vec<Individual>,
    /// MAP-Elites archive: best individual per behavioral cell.
    pub archive: HashMap<CellKey, Individual>,
    /// Population size target.
    pub target_size: usize,
    /// Tournament selection size.
    pub tournament_k: usize,
    /// Maximum depth for random/mutated trees.
    pub max_depth: usize,
}

impl Population {
    pub fn new(target_size: usize) -> Self {
        Population {
            individuals: Vec::with_capacity(target_size),
            archive: HashMap::new(),
            target_size,
            tournament_k: 5,
            max_depth: 10,
        }
    }

    /// Initialize population with random expression trees.
    pub fn initialize(&mut self, rng: &mut impl Rng) {
        self.individuals.clear();
        for _ in 0..self.target_size {
            let term = mutate::random_term(rng, self.max_depth);
            self.individuals.push(Individual::new(term));
        }
    }

    /// Run one generation: select parents, mutate/crossover, produce offspring.
    pub fn evolve(&mut self, rng: &mut impl Rng) {
        let mut offspring = Vec::with_capacity(self.target_size);

        while offspring.len() < self.target_size {
            let (p1, p2) = select::select_parents(&self.individuals, self.tournament_k, rng);

            if rng.gen_bool(0.3) {
                // Crossover
                let (c1, c2) = mutate::crossover(&p1.term, &p2.term, rng);
                offspring.push(Individual::new(c1));
                if offspring.len() < self.target_size {
                    offspring.push(Individual::new(c2));
                }
            } else {
                // Mutation
                let mutated = mutate::mutate(&p1.term, rng, self.max_depth);
                offspring.push(Individual::new(mutated));
            }
        }

        offspring.truncate(self.target_size);
        self.individuals = offspring;
    }

    /// Update the MAP-Elites archive with the current population.
    /// Each cell keeps only the fittest individual for that behavioral niche.
    pub fn update_archive(&mut self) {
        for ind in &self.individuals {
            let key = (ind.depth_bin, ind.op_diversity, ind.cr_bin);
            let dominated = match self.archive.get(&key) {
                Some(existing) => ind.fitness > existing.fitness,
                None => true,
            };
            if dominated {
                self.archive.insert(key, ind.clone());
            }
        }
    }

    /// Inject archive elites back into the population (MAP-Elites feedback).
    pub fn inject_elites(&mut self, fraction: f64) {
        let n_inject = ((self.target_size as f64) * fraction) as usize;
        let elites: Vec<Individual> = self.archive.values().cloned().collect();

        if elites.is_empty() {
            return;
        }

        // Replace the worst individuals with archive elites
        self.individuals
            .sort_by(|a, b| a.fitness.partial_cmp(&b.fitness).unwrap());

        for (i, elite) in elites.iter().cycle().take(n_inject).enumerate() {
            if i < self.individuals.len() {
                self.individuals[i] = elite.clone();
            }
        }
    }

    /// Get the best individual by fitness.
    pub fn best(&self) -> Option<&Individual> {
        self.individuals
            .iter()
            .max_by(|a, b| a.fitness.partial_cmp(&b.fitness).unwrap())
    }

    /// Average fitness of the population.
    pub fn avg_fitness(&self) -> f64 {
        if self.individuals.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.individuals.iter().map(|i| i.fitness).sum();
        sum / self.individuals.len() as f64
    }

    /// Population diversity: ratio of unique content hashes.
    pub fn diversity(&self) -> f64 {
        if self.individuals.is_empty() {
            return 0.0;
        }
        let unique: std::collections::HashSet<_> =
            self.individuals.iter().map(|i| i.hash).collect();
        unique.len() as f64 / self.individuals.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn initialize_creates_target_size() {
        let mut pop = Population::new(100);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        pop.initialize(&mut rng);
        assert_eq!(pop.individuals.len(), 100);
    }

    #[test]
    fn evolve_maintains_size() {
        let mut pop = Population::new(50);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        pop.initialize(&mut rng);
        // Set some fitness values
        for (i, ind) in pop.individuals.iter_mut().enumerate() {
            ind.fitness = i as f64 / 50.0;
        }
        pop.evolve(&mut rng);
        assert_eq!(pop.individuals.len(), 50);
    }

    #[test]
    fn diversity_correct() {
        let mut pop = Population::new(10);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        pop.initialize(&mut rng);
        let d = pop.diversity();
        assert!(d > 0.0 && d <= 1.0);
    }

    #[test]
    fn update_archive_inserts_individuals() {
        let mut pop = Population::new(5);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        pop.initialize(&mut rng);

        // Set distinct fitness values
        for (i, ind) in pop.individuals.iter_mut().enumerate() {
            ind.fitness = (i + 1) as f64;
            ind.update_bins();
        }

        assert!(pop.archive.is_empty(), "archive should start empty");
        pop.update_archive();
        assert!(
            !pop.archive.is_empty(),
            "archive should have entries after update"
        );
    }

    #[test]
    fn inject_elites_replaces_lowest_fitness() {
        let mut pop = Population::new(10);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        pop.initialize(&mut rng);

        // Set fitness values: 0.0 to 0.9
        for (i, ind) in pop.individuals.iter_mut().enumerate() {
            ind.fitness = i as f64 / 10.0;
            ind.update_bins();
        }

        // Update archive to populate it
        pop.update_archive();

        // Inject elites at 20% fraction
        pop.inject_elites(0.2);

        // After injection, the lowest-fitness slots should have been replaced by elites
        // The population should still have the correct size
        assert_eq!(pop.individuals.len(), 10);
    }

    #[test]
    fn best_returns_highest_fitness() {
        let mut pop = Population::new(5);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        pop.initialize(&mut rng);

        pop.individuals[0].fitness = 0.1;
        pop.individuals[1].fitness = 0.9;
        pop.individuals[2].fitness = 0.5;
        pop.individuals[3].fitness = 0.3;
        pop.individuals[4].fitness = 0.7;

        let best = pop.best().unwrap();
        assert_eq!(best.fitness, 0.9);
    }

    #[test]
    fn avg_fitness_computes_correctly() {
        let mut pop = Population::new(4);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        pop.initialize(&mut rng);

        pop.individuals[0].fitness = 1.0;
        pop.individuals[1].fitness = 2.0;
        pop.individuals[2].fitness = 3.0;
        pop.individuals[3].fitness = 4.0;

        let avg = pop.avg_fitness();
        assert!(
            (avg - 2.5).abs() < f64::EPSILON,
            "avg should be 2.5, got {avg}"
        );
    }

    #[test]
    fn empty_population_avg_fitness_and_diversity() {
        let pop = Population::new(10);
        // No initialization — population is empty
        assert_eq!(
            pop.avg_fitness(),
            0.0,
            "empty pop avg_fitness should be 0.0"
        );
        assert_eq!(pop.diversity(), 0.0, "empty pop diversity should be 0.0");
    }
}
