import { Box, Card, CardContent, Typography } from "@mui/material";
import {
	CartesianGrid,
	Legend,
	Line,
	LineChart,
	ResponsiveContainer,
	Tooltip,
	XAxis,
	YAxis,
} from "recharts";
import { useEpochs } from "../../hooks/useEngine";
import { tokens } from "../../theme";

export function EpochChart() {
	const { data, isLoading } = useEpochs(100, 0);

	if (isLoading || !data) {
		return (
			<Card>
				<CardContent>
					<Typography color="text.secondary">Loading epochs...</Typography>
				</CardContent>
			</Card>
		);
	}

	// Reverse so oldest is first (chart reads left-to-right)
	const chartData = [...data.epochs].reverse();

	return (
		<Card>
			<CardContent>
				<Typography variant="h3" sx={{ mb: 2 }}>
					Epoch Timeline
				</Typography>
				<Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
					{data.total} epochs recorded
				</Typography>

				{chartData.length === 0 ? (
					<Box
						sx={{
							height: 300,
							display: "flex",
							alignItems: "center",
							justifyContent: "center",
						}}
					>
						<Typography color="text.secondary">
							No epochs yet — engine is starting...
						</Typography>
					</Box>
				) : (
					<ResponsiveContainer width="100%" height={300}>
						<LineChart data={chartData}>
							<CartesianGrid
								strokeDasharray="3 3"
								stroke={tokens.border.default}
							/>
							<XAxis
								dataKey="epoch"
								stroke={tokens.text.secondary}
								fontSize={11}
							/>
							<YAxis
								stroke={tokens.text.secondary}
								fontSize={11}
							/>
							<Tooltip
								contentStyle={{
									backgroundColor: tokens.background.elevated,
									border: `1px solid ${tokens.border.subtle}`,
									borderRadius: tokens.borderRadius.md,
									fontSize: 12,
								}}
							/>
							<Legend
								wrapperStyle={{ fontSize: 12 }}
							/>
							<Line
								type="monotone"
								dataKey="compression_ratio"
								name="Compression"
								stroke={tokens.brand.emerald}
								strokeWidth={2}
								dot={false}
							/>
							<Line
								type="monotone"
								dataKey="novelty_total"
								name="Novelty"
								stroke={tokens.brand.rose}
								strokeWidth={2}
								dot={false}
							/>
							<Line
								type="monotone"
								dataKey="meta_compression"
								name="Meta-CR"
								stroke={tokens.brand.accent}
								strokeWidth={1.5}
								dot={false}
								strokeDasharray="4 2"
							/>
						</LineChart>
					</ResponsiveContainer>
				)}

				{chartData.length > 0 && (
					<Box sx={{ mt: 2 }}>
						<ResponsiveContainer width="100%" height={200}>
							<LineChart data={chartData}>
								<CartesianGrid
									strokeDasharray="3 3"
									stroke={tokens.border.default}
								/>
								<XAxis
									dataKey="epoch"
									stroke={tokens.text.secondary}
									fontSize={11}
								/>
								<YAxis
									stroke={tokens.text.secondary}
									fontSize={11}
								/>
								<Tooltip
									contentStyle={{
										backgroundColor: tokens.background.elevated,
										border: `1px solid ${tokens.border.subtle}`,
										borderRadius: tokens.borderRadius.md,
										fontSize: 12,
									}}
								/>
								<Legend wrapperStyle={{ fontSize: 12 }} />
								<Line
									type="monotone"
									dataKey="library_size"
									name="Library Size"
									stroke={tokens.brand.accent}
									strokeWidth={2}
									dot={false}
								/>
								<Line
									type="monotone"
									dataKey="population_diversity"
									name="Diversity"
									stroke={tokens.brand.secondary}
									strokeWidth={2}
									dot={false}
								/>
							</LineChart>
						</ResponsiveContainer>
					</Box>
				)}
			</CardContent>
		</Card>
	);
}
