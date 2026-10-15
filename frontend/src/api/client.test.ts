import { describe, expect, it, vi, beforeEach } from "vitest";
import { api } from "./client";
import { mockStatus, mockEpochList, mockLibraryList, mockConfig } from "../test/fixtures";

const mockFetch = vi.fn();
vi.stubGlobal("fetch", mockFetch);

function mockJsonResponse(data: unknown) {
	return {
		ok: true,
		json: () => Promise.resolve(data),
	};
}

beforeEach(() => {
	mockFetch.mockReset();
});

describe("api client", () => {
	it("fetches status", async () => {
		mockFetch.mockResolvedValueOnce(mockJsonResponse(mockStatus));
		const result = await api.getStatus();
		expect(result.epoch).toBe(42);
		expect(result.running).toBe(true);
		expect(mockFetch).toHaveBeenCalledWith(
			expect.stringContaining("/api/status"),
			expect.any(Object),
		);
	});

	it("fetches epochs with pagination", async () => {
		mockFetch.mockResolvedValueOnce(mockJsonResponse(mockEpochList));
		const result = await api.getEpochs(10, 5);
		expect(result.total).toBe(10);
		expect(mockFetch).toHaveBeenCalledWith(
			expect.stringContaining("/api/epochs?limit=10&offset=5"),
			expect.any(Object),
		);
	});

	it("fetches library", async () => {
		mockFetch.mockResolvedValueOnce(mockJsonResponse(mockLibraryList));
		const result = await api.getLibrary();
		expect(result.symbols).toHaveLength(2);
	});

	it("fetches config", async () => {
		mockFetch.mockResolvedValueOnce(mockJsonResponse(mockConfig));
		const result = await api.getConfig();
		expect(result.population_size).toBe(10000);
		expect(result.alpha).toBe(0.6);
	});

	it("sends pause request", async () => {
		mockFetch.mockResolvedValueOnce(
			mockJsonResponse({ success: true, message: "paused" }),
		);
		const result = await api.pause();
		expect(result.success).toBe(true);
		expect(mockFetch).toHaveBeenCalledWith(
			expect.stringContaining("/api/engine/pause"),
			expect.objectContaining({ method: "POST" }),
		);
	});

	it("sends resume request", async () => {
		mockFetch.mockResolvedValueOnce(
			mockJsonResponse({ success: true, message: "resumed" }),
		);
		const result = await api.resume();
		expect(result.success).toBe(true);
	});

	it("sends config update", async () => {
		mockFetch.mockResolvedValueOnce(
			mockJsonResponse({ success: true, message: "updated" }),
		);
		const result = await api.updateConfig({ alpha: 0.8, running: false });
		expect(result.success).toBe(true);
		expect(mockFetch).toHaveBeenCalledWith(
			expect.stringContaining("/api/config"),
			expect.objectContaining({
				method: "PUT",
				body: JSON.stringify({ alpha: 0.8, running: false }),
			}),
		);
	});

	it("throws on HTTP error", async () => {
		mockFetch.mockResolvedValueOnce({ ok: false, status: 500, statusText: "Internal Server Error" });
		await expect(api.getStatus()).rejects.toThrow("API error: 500");
	});
});
