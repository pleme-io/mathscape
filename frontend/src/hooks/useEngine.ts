/** React Query hooks for the engine API. */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../api/client";
import type { ConfigUpdate } from "../types/api";

export const queryKeys = {
	status: ["engine", "status"] as const,
	epochs: (limit: number, offset: number) =>
		["engine", "epochs", limit, offset] as const,
	library: ["engine", "library"] as const,
	config: ["engine", "config"] as const,
};

/** Live engine status — polls every 1s. */
export function useStatus() {
	return useQuery({
		queryKey: queryKeys.status,
		queryFn: api.getStatus,
		refetchInterval: 1000,
	});
}

/** Epoch history — polls every 2s. */
export function useEpochs(limit = 50, offset = 0) {
	return useQuery({
		queryKey: queryKeys.epochs(limit, offset),
		queryFn: () => api.getEpochs(limit, offset),
		refetchInterval: 2000,
	});
}

/** Library symbols — polls every 5s. */
export function useLibrary() {
	return useQuery({
		queryKey: queryKeys.library,
		queryFn: api.getLibrary,
		refetchInterval: 5000,
	});
}

/** Engine config — fetched once then invalidated on mutations. */
export function useConfig() {
	return useQuery({
		queryKey: queryKeys.config,
		queryFn: api.getConfig,
	});
}

/** Pause/resume engine mutations. */
export function useEngineControl() {
	const qc = useQueryClient();
	const invalidate = () => {
		qc.invalidateQueries({ queryKey: queryKeys.status });
		qc.invalidateQueries({ queryKey: queryKeys.config });
	};

	const pause = useMutation({
		mutationFn: api.pause,
		onSuccess: invalidate,
	});

	const resume = useMutation({
		mutationFn: api.resume,
		onSuccess: invalidate,
	});

	const updateConfig = useMutation({
		mutationFn: (update: ConfigUpdate) => api.updateConfig(update),
		onSuccess: invalidate,
	});

	return { pause, resume, updateConfig };
}
