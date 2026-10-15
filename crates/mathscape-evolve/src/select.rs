//! Selection operators: tournament selection for evolutionary search.

use crate::individual::Individual;
use rand::Rng;

/// Tournament selection: pick `k` random individuals, return the fittest.
pub fn tournament<'a>(
    population: &'a [Individual],
    k: usize,
    rng: &mut impl Rng,
) -> &'a Individual {
    assert!(!population.is_empty(), "population cannot be empty");
    let k = k.min(population.len());

    let mut best_idx = rng.gen_range(0..population.len());
    for _ in 1..k {
        let idx = rng.gen_range(0..population.len());
        if population[idx].fitness > population[best_idx].fitness {
            best_idx = idx;
        }
    }
    &population[best_idx]
}

/// Select a pair of parents via tournament selection.
pub fn select_parents<'a>(
    population: &'a [Individual],
    tournament_size: usize,
    rng: &mut impl Rng,
) -> (&'a Individual, &'a Individual) {
    let p1 = tournament(population, tournament_size, rng);
    let p2 = tournament(population, tournament_size, rng);
    (p1, p2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::term::Term;
    use mathscape_core::value::Value;
    use rand::SeedableRng;

    fn make_pop(fitnesses: &[f64]) -> Vec<Individual> {
        fitnesses
            .iter()
            .map(|&f| {
                let mut ind = Individual::new(Term::Number(Value::Nat(0)));
                ind.fitness = f;
                ind
            })
            .collect()
    }

    #[test]
    fn tournament_returns_fittest_with_high_k() {
        let pop = make_pop(&[0.1, 0.5, 0.9, 0.3]);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        // With k=pop.len(), should always return the fittest
        let winner = tournament(&pop, pop.len(), &mut rng);
        assert_eq!(winner.fitness, 0.9);
    }

    #[test]
    fn select_parents_returns_two_individuals() {
        let pop = make_pop(&[0.1, 0.5, 0.9, 0.3, 0.7]);
        let mut rng = rand::rngs::StdRng::seed_from_u64(123);
        let (p1, p2) = select_parents(&pop, 3, &mut rng);
        // Both parents should have fitness values from the population
        assert!(p1.fitness >= 0.0);
        assert!(p2.fitness >= 0.0);
        // Fitness should be one of the values in our population
        let valid_fitnesses = [0.1, 0.5, 0.9, 0.3, 0.7];
        assert!(
            valid_fitnesses
                .iter()
                .any(|&f| (f - p1.fitness).abs() < f64::EPSILON),
            "parent1 fitness should come from population"
        );
        assert!(
            valid_fitnesses
                .iter()
                .any(|&f| (f - p2.fitness).abs() < f64::EPSILON),
            "parent2 fitness should come from population"
        );
    }

    #[test]
    fn tournament_with_k1_returns_valid_individual() {
        let pop = make_pop(&[0.2, 0.8, 0.4, 0.6]);
        let mut rng = rand::rngs::StdRng::seed_from_u64(55);
        // k=1 means pick one random individual (no comparison)
        let winner = tournament(&pop, 1, &mut rng);
        let valid_fitnesses = [0.2, 0.8, 0.4, 0.6];
        assert!(
            valid_fitnesses
                .iter()
                .any(|&f| (f - winner.fitness).abs() < f64::EPSILON),
            "tournament with k=1 should return a valid individual from the population"
        );
    }
}
