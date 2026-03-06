import { Box, Container, CssBaseline, Typography } from "@mui/material";
import { ThemeProvider } from "@mui/material/styles";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StatusPanel } from "./components/dashboard/StatusPanel";
import { EpochChart } from "./components/epochs/EpochChart";
import { LibraryTable } from "./components/library/LibraryTable";
import { ConfigPanel } from "./components/config/ConfigPanel";
import { theme, tokens } from "./theme";

const queryClient = new QueryClient({
	defaultOptions: {
		queries: {
			staleTime: 1000,
			retry: 1,
			refetchOnWindowFocus: false,
		},
	},
});

function AppContent() {
	return (
		<Box
			sx={{
				minHeight: "100vh",
				backgroundColor: tokens.background.default,
				py: 3,
			}}
		>
			<Container maxWidth="lg">
				<Box sx={{ mb: 4 }}>
					<Typography
						variant="h1"
						sx={{
							background: `linear-gradient(135deg, ${tokens.brand.primary}, ${tokens.brand.accent})`,
							WebkitBackgroundClip: "text",
							WebkitTextFillColor: "transparent",
							display: "inline-block",
						}}
					>
						Mathscape
					</Typography>
					<Typography variant="body2" color="text.secondary">
						Evolutionary symbolic compression engine
					</Typography>
				</Box>

				<Box sx={{ mb: 3 }}>
					<StatusPanel />
				</Box>

				<Box sx={{ mb: 3 }}>
					<EpochChart />
				</Box>

				<Box
					sx={{
						display: "grid",
						gridTemplateColumns: { xs: "1fr", md: "1fr 1fr" },
						gap: 3,
						mb: 3,
					}}
				>
					<LibraryTable />
					<ConfigPanel />
				</Box>
			</Container>
		</Box>
	);
}

export default function App() {
	return (
		<ThemeProvider theme={theme}>
			<CssBaseline />
			<QueryClientProvider client={queryClient}>
				<AppContent />
			</QueryClientProvider>
		</ThemeProvider>
	);
}
