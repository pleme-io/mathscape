import { describe, expect, it } from "vitest";
import type {
	Status,
	EpochMetrics,
	LibrarySymbol,
	EngineConfig,
	ConfigUpdate,
	ControlResponse,
} from "./api";
import {
	mockStatus,
	mockConfig,
	mockEpochMetrics,
	mockLibrarySymbols,
} from "../test/fixtures";

describe("API types", () => {
	it("Status has all required fields", () => {
		const status: Status = mockStatus;
		expect(status.epoch).toBeTypeOf("number");
		expect(status.running).toBeTypeOf("boolean");
		expect(status.library_size).toBeTypeOf("number");
		expect(status.population_size).toBeTypeOf("number");
		expect(status.avg_fitness).toBeTypeOf("number");
		expect(status.diversity).toBeTypeOf("number");
		expect(status.latest_reward).not.toBeNull();
	});

	it("EpochMetrics has all required fields", () => {
		const metrics: EpochMetrics = mockEpochMetrics[0]!;
		expect(metrics.epoch).toBeTypeOf("number");
		expect(metrics.compression_ratio).toBeTypeOf("number");
		expect(metrics.alpha).toBeTypeOf("number");
		expect(metrics.beta).toBeTypeOf("number");
		expect(metrics.gamma).toBeTypeOf("number");
	});

	it("LibrarySymbol has all required fields", () => {
		const sym: LibrarySymbol = mockLibrarySymbols[0]!;
		expect(sym.symbol_id).toBeTypeOf("number");
		expect(sym.name).toBeTypeOf("string");
		expect(sym.lhs_sexpr).toBeTypeOf("string");
		expect(sym.rhs_sexpr).toBeTypeOf("string");
		expect(sym.is_meta).toBeTypeOf("boolean");
	});

	it("EngineConfig has all required fields", () => {
		const config: EngineConfig = mockConfig;
		expect(config.population_size).toBeGreaterThan(0);
		expect(config.alpha + config.beta + config.gamma).toBeCloseTo(1.0);
	});

	it("ConfigUpdate allows partial fields", () => {
		const update: ConfigUpdate = { alpha: 0.8 };
		expect(update.alpha).toBe(0.8);
		expect(update.beta).toBeUndefined();
		expect(update.running).toBeUndefined();
	});

	it("ControlResponse has success and message", () => {
		const resp: ControlResponse = { success: true, message: "ok" };
		expect(resp.success).toBe(true);
		expect(resp.message).toBe("ok");
	});

	it("Status serializes/deserializes via JSON", () => {
		const json = JSON.stringify(mockStatus);
		const restored: Status = JSON.parse(json);
		expect(restored.epoch).toBe(mockStatus.epoch);
		expect(restored.latest_reward?.compression_ratio).toBe(
			mockStatus.latest_reward?.compression_ratio,
		);
	});
});
