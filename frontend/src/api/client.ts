/** REST API client for the mathscape-service backend. */

import type {
	ConfigUpdate,
	ControlResponse,
	EngineConfig,
	EpochList,
	LibraryList,
	Status,
} from "../types/api";

const BASE_URL = import.meta.env.VITE_API_URL ?? "http://localhost:8080";

async function fetchJson<T>(path: string, init?: RequestInit): Promise<T> {
	const res = await fetch(`${BASE_URL}${path}`, {
		headers: { "Content-Type": "application/json" },
		...init,
	});
	if (!res.ok) {
		throw new Error(`API error: ${res.status} ${res.statusText}`);
	}
	return res.json();
}

export const api = {
	getStatus: () => fetchJson<Status>("/api/status"),

	getEpochs: (limit = 50, offset = 0) =>
		fetchJson<EpochList>(`/api/epochs?limit=${limit}&offset=${offset}`),

	getLibrary: () => fetchJson<LibraryList>("/api/library"),

	getConfig: () => fetchJson<EngineConfig>("/api/config"),

	updateConfig: (update: ConfigUpdate) =>
		fetchJson<ControlResponse>("/api/config", {
			method: "PUT",
			body: JSON.stringify(update),
		}),

	pause: () =>
		fetchJson<ControlResponse>("/api/engine/pause", { method: "POST" }),

	resume: () =>
		fetchJson<ControlResponse>("/api/engine/resume", { method: "POST" }),
} as const;
