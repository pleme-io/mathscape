import {
	Box,
	Button,
	Card,
	CardContent,
	Grid,
	Slider,
	TextField,
	Typography,
} from "@mui/material";
import { Settings, Save } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useConfig, useEngineControl } from "../../hooks/useEngine";
import { tokens } from "../../theme";
import type { ConfigUpdate } from "../../types/api";

export function ConfigPanel() {
	const { data: config, isLoading } = useConfig();
	const { updateConfig } = useEngineControl();

	const [draft, setDraft] = useState<ConfigUpdate>({});
	const [dirty, setDirty] = useState(false);

	useEffect(() => {
		if (config) {
			setDraft({
				alpha: config.alpha,
				beta: config.beta,
				gamma: config.gamma,
				population_size: config.population_size,
				tournament_k: config.tournament_k,
				max_depth: config.max_depth,
				epoch_delay_ms: config.epoch_delay_ms,
			});
			setDirty(false);
		}
	}, [config]);

	const update = useCallback(
		(field: keyof ConfigUpdate, value: number) => {
			setDraft((prev) => ({ ...prev, [field]: value }));
			setDirty(true);
		},
		[],
	);

	const handleSave = useCallback(() => {
		updateConfig.mutate(draft, {
			onSuccess: () => setDirty(false),
		});
	}, [draft, updateConfig]);

	if (isLoading || !config) {
		return (
			<Card>
				<CardContent>
					<Typography color="text.secondary">Loading config...</Typography>
				</CardContent>
			</Card>
		);
	}

	return (
		<Card>
			<CardContent>
				<Box
					sx={{
						display: "flex",
						alignItems: "center",
						justifyContent: "space-between",
						mb: 3,
					}}
				>
					<Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
						<Settings size={18} color={tokens.text.secondary} />
						<Typography variant="h3">Configuration</Typography>
					</Box>
					{dirty && (
						<Button
							variant="contained"
							size="small"
							startIcon={<Save size={14} />}
							onClick={handleSave}
							disabled={updateConfig.isPending}
						>
							Apply
						</Button>
					)}
				</Box>

				<Typography
					variant="caption"
					color="text.secondary"
					sx={{ mb: 2, display: "block" }}
				>
					Reward Weights (alpha + beta + gamma)
				</Typography>
				<Grid container spacing={3} sx={{ mb: 3 }}>
					<Grid size={{ xs: 4 }}>
						<Typography variant="body2" sx={{ mb: 1 }}>
							Alpha (Compression): {draft.alpha?.toFixed(2)}
						</Typography>
						<Slider
							value={draft.alpha ?? config.alpha}
							min={0}
							max={1}
							step={0.05}
							onChange={(_, v) => update("alpha", v as number)}
							sx={{ color: tokens.brand.emerald }}
						/>
					</Grid>
					<Grid size={{ xs: 4 }}>
						<Typography variant="body2" sx={{ mb: 1 }}>
							Beta (Novelty): {draft.beta?.toFixed(2)}
						</Typography>
						<Slider
							value={draft.beta ?? config.beta}
							min={0}
							max={1}
							step={0.05}
							onChange={(_, v) => update("beta", v as number)}
							sx={{ color: tokens.brand.rose }}
						/>
					</Grid>
					<Grid size={{ xs: 4 }}>
						<Typography variant="body2" sx={{ mb: 1 }}>
							Gamma (Meta-CR): {draft.gamma?.toFixed(2)}
						</Typography>
						<Slider
							value={draft.gamma ?? config.gamma}
							min={0}
							max={1}
							step={0.05}
							onChange={(_, v) => update("gamma", v as number)}
							sx={{ color: tokens.brand.accent }}
						/>
					</Grid>
				</Grid>

				<Typography
					variant="caption"
					color="text.secondary"
					sx={{ mb: 2, display: "block" }}
				>
					Population Parameters
				</Typography>
				<Grid container spacing={3} sx={{ mb: 3 }}>
					<Grid size={{ xs: 4 }}>
						<TextField
							label="Population Size"
							type="number"
							size="small"
							fullWidth
							value={draft.population_size ?? config.population_size}
							onChange={(e) =>
								update("population_size", parseInt(e.target.value, 10))
							}
						/>
					</Grid>
					<Grid size={{ xs: 4 }}>
						<TextField
							label="Tournament K"
							type="number"
							size="small"
							fullWidth
							value={draft.tournament_k ?? config.tournament_k}
							onChange={(e) =>
								update("tournament_k", parseInt(e.target.value, 10))
							}
						/>
					</Grid>
					<Grid size={{ xs: 4 }}>
						<TextField
							label="Max Depth"
							type="number"
							size="small"
							fullWidth
							value={draft.max_depth ?? config.max_depth}
							onChange={(e) =>
								update("max_depth", parseInt(e.target.value, 10))
							}
						/>
					</Grid>
				</Grid>

				<Typography
					variant="caption"
					color="text.secondary"
					sx={{ mb: 2, display: "block" }}
				>
					Engine Control
				</Typography>
				<Grid container spacing={3}>
					<Grid size={{ xs: 6 }}>
						<TextField
							label="Epoch Delay (ms)"
							type="number"
							size="small"
							fullWidth
							value={draft.epoch_delay_ms ?? config.epoch_delay_ms}
							onChange={(e) =>
								update("epoch_delay_ms", parseInt(e.target.value, 10))
							}
						/>
					</Grid>
					<Grid size={{ xs: 6 }}>
						<Box>
							<Typography variant="body2" color="text.secondary">
								Elite Fraction: {config.elite_fraction}
							</Typography>
							<Typography variant="body2" color="text.secondary">
								Crossover Rate: {config.crossover_rate}
							</Typography>
						</Box>
					</Grid>
				</Grid>
			</CardContent>
		</Card>
	);
}
