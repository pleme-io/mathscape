import {
	Box,
	Card,
	CardContent,
	Chip,
	Grid,
	IconButton,
	Tooltip,
	Typography,
} from "@mui/material";
import { Pause, Play, Activity, Layers, Users, Sparkles } from "lucide-react";
import { useStatus, useEngineControl } from "../../hooks/useEngine";
import { tokens } from "../../theme";

function MetricCard({
	label,
	value,
	icon,
	color,
}: {
	label: string;
	value: string | number;
	icon: React.ReactNode;
	color: string;
}) {
	return (
		<Card sx={{ height: "100%" }}>
			<CardContent sx={{ p: 2, "&:last-child": { pb: 2 } }}>
				<Box sx={{ display: "flex", alignItems: "center", gap: 1, mb: 1 }}>
					<Box sx={{ color, display: "flex" }}>{icon}</Box>
					<Typography variant="caption" color="text.secondary">
						{label}
					</Typography>
				</Box>
				<Typography variant="h3" sx={{ fontVariantNumeric: "tabular-nums" }}>
					{value}
				</Typography>
			</CardContent>
		</Card>
	);
}

export function StatusPanel() {
	const { data: status, isLoading } = useStatus();
	const { pause, resume } = useEngineControl();

	if (isLoading || !status) {
		return (
			<Card>
				<CardContent>
					<Typography color="text.secondary">Connecting...</Typography>
				</CardContent>
			</Card>
		);
	}

	const isRunning = status.running;

	return (
		<Box>
			<Box
				sx={{
					display: "flex",
					alignItems: "center",
					justifyContent: "space-between",
					mb: 2,
				}}
			>
				<Box sx={{ display: "flex", alignItems: "center", gap: 1.5 }}>
					<Typography variant="h2">Engine</Typography>
					<Chip
						label={isRunning ? "Running" : "Paused"}
						color={isRunning ? "success" : "warning"}
						size="small"
						variant="outlined"
					/>
				</Box>
				<Tooltip title={isRunning ? "Pause engine" : "Resume engine"}>
					<IconButton
						onClick={() =>
							isRunning ? pause.mutate() : resume.mutate()
						}
						color={isRunning ? "warning" : "success"}
						disabled={pause.isPending || resume.isPending}
					>
						{isRunning ? <Pause size={20} /> : <Play size={20} />}
					</IconButton>
				</Tooltip>
			</Box>

			<Grid container spacing={2}>
				<Grid size={{ xs: 6, sm: 3 }}>
					<MetricCard
						label="Epoch"
						value={status.epoch}
						icon={<Activity size={16} />}
						color={tokens.brand.primary}
					/>
				</Grid>
				<Grid size={{ xs: 6, sm: 3 }}>
					<MetricCard
						label="Library"
						value={status.library_size}
						icon={<Layers size={16} />}
						color={tokens.brand.accent}
					/>
				</Grid>
				<Grid size={{ xs: 6, sm: 3 }}>
					<MetricCard
						label="Population"
						value={status.population_size.toLocaleString()}
						icon={<Users size={16} />}
						color={tokens.brand.emerald}
					/>
				</Grid>
				<Grid size={{ xs: 6, sm: 3 }}>
					<MetricCard
						label="Diversity"
						value={`${(status.diversity * 100).toFixed(1)}%`}
						icon={<Sparkles size={16} />}
						color={tokens.brand.secondary}
					/>
				</Grid>
			</Grid>

			{status.latest_reward && (
				<Card sx={{ mt: 2 }}>
					<CardContent sx={{ p: 2, "&:last-child": { pb: 2 } }}>
						<Typography variant="caption" color="text.secondary" sx={{ mb: 1, display: "block" }}>
							Latest Reward Breakdown
						</Typography>
						<Grid container spacing={2}>
							<Grid size={{ xs: 4 }}>
								<Typography variant="body2" color="text.secondary">
									Compression
								</Typography>
								<Typography
									variant="h4"
									sx={{ color: tokens.brand.emerald }}
								>
									{status.latest_reward.compression_ratio.toFixed(4)}
								</Typography>
							</Grid>
							<Grid size={{ xs: 4 }}>
								<Typography variant="body2" color="text.secondary">
									Novelty
								</Typography>
								<Typography
									variant="h4"
									sx={{ color: tokens.brand.rose }}
								>
									{status.latest_reward.novelty_total.toFixed(4)}
								</Typography>
							</Grid>
							<Grid size={{ xs: 4 }}>
								<Typography variant="body2" color="text.secondary">
									Meta-CR
								</Typography>
								<Typography
									variant="h4"
									sx={{ color: tokens.brand.accent }}
								>
									{status.latest_reward.meta_compression.toFixed(4)}
								</Typography>
							</Grid>
						</Grid>
					</CardContent>
				</Card>
			)}
		</Box>
	);
}
