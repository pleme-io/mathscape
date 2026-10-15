/** Central API types — mirrors the Rust mathscape-api crate. */

export interface RewardSnapshot {
	reward: number;
	compression_ratio: number;
	description_length: number;
	raw_length: number;
	novelty_total: number;
	meta_compression: number;
}

export interface Status {
	epoch: number;
	running: boolean;
	library_size: number;
	population_size: number;
	avg_fitness: number;
	diversity: number;
	latest_reward: RewardSnapshot | null;
}

export interface EpochMetrics {
	epoch: number;
	compression_ratio: number;
	description_length: number;
	novelty_total: number;
	meta_compression: number;
	library_size: number;
	population_diversity: number | null;
	phase: string | null;
	duration_ms: number | null;
	alpha: number;
	beta: number;
	gamma: number;
}

export interface LibrarySymbol {
	symbol_id: number;
	name: string;
	epoch_discovered: number;
	arity: number;
	lhs_sexpr: string;
	rhs_sexpr: string;
	generality: number | null;
	irreducibility: number | null;
	is_meta: boolean;
	status: string;
}

export interface EngineConfig {
	running: boolean;
	max_epoch: number | null;
	epoch_delay_ms: number;
	alpha: number;
	beta: number;
	gamma: number;
	population_size: number;
	tournament_k: number;
	max_depth: number;
	elite_fraction: number;
	crossover_rate: number;
	min_shared_size: number;
	min_matches: number;
	max_new_rules: number;
}

export interface ConfigUpdate {
	running?: boolean;
	max_epoch?: number;
	epoch_delay_ms?: number;
	alpha?: number;
	beta?: number;
	gamma?: number;
	population_size?: number;
	tournament_k?: number;
	max_depth?: number;
}

export interface ControlResponse {
	success: boolean;
	message: string;
}

export interface EpochList {
	epochs: EpochMetrics[];
	total: number;
}

export interface LibraryList {
	symbols: LibrarySymbol[];
}
