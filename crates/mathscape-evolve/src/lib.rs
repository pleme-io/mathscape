//! Genetic operators, population management, tournament selection, MAP-Elites.

pub mod individual;
pub mod mutate;
pub mod population;
pub mod select;

pub use individual::Individual;
pub use population::Population;
