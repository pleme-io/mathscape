/** Test fixtures — sample API responses for component tests. */

import type {
	Status,
	EpochMetrics,
	LibrarySymbol,
	EngineConfig,
	EpochList,
	LibraryList,
} from "../types/api";

export const mockStatus: Status = {
	epoch: 42,
	running: true,
	library_size: 15,
	population_size: 10000,
	avg_fitness: 0.78,
	diversity: 0.65,
	latest_reward: {
		reward: 1.5,
		compression_ratio: 0.42,
		description_length: 100,
		raw_length: 238,
		novelty_total: 0.87,
		meta_compression: 0.33,
	},
};

export const mockEpochMetrics: EpochMetrics[] = Array.from({ length: 10 }, (_, i) => ({
	epoch: i + 1,
	compression_ratio: 0.1 + i * 0.03,
	description_length: 200 - i * 10,
	novelty_total: 0.5 + Math.sin(i) * 0.2,
	meta_compression: 0.1 + i * 0.02,
	library_size: i + 1,
	population_diversity: 0.7 - i * 0.02,
	phase: i < 5 ? "exploration" : "exploitation",
	duration_ms: 100 + i * 10,
	alpha: 0.6,
	beta: 0.3,
	gamma: 0.1,
}));

export const mockEpochList: EpochList = {
	epochs: mockEpochMetrics,
	total: 10,
};

export const mockLibrarySymbols: LibrarySymbol[] = [
	{
		symbol_id: 0,
		name: "S_001",
		epoch_discovered: 3,
		arity: 2,
		lhs_sexpr: "(add ?100 0)",
		rhs_sexpr: "?100",
		generality: 0.8,
		irreducibility: 1.0,
		is_meta: false,
		status: "active",
	},
	{
		symbol_id: 1,
		name: "S_002",
		epoch_discovered: 7,
		arity: 2,
		lhs_sexpr: "(add ?100 ?101)",
		rhs_sexpr: "(add ?101 ?100)",
		generality: 0.9,
		irreducibility: 0.95,
		is_meta: false,
		status: "active",
	},
];

export const mockLibraryList: LibraryList = {
	symbols: mockLibrarySymbols,
};

export const mockConfig: EngineConfig = {
	running: true,
	max_epoch: null,
	epoch_delay_ms: 0,
	alpha: 0.6,
	beta: 0.3,
	gamma: 0.1,
	population_size: 10000,
	tournament_k: 5,
	max_depth: 10,
	elite_fraction: 0.1,
	crossover_rate: 0.3,
	min_shared_size: 3,
	min_matches: 2,
	max_new_rules: 5,
};
