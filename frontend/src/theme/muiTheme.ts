import { createTheme } from "@mui/material/styles";
import { tokens } from "./tokens";

export const theme = createTheme({
	cssVariables: true,
	palette: {
		mode: "dark",
		primary: {
			main: tokens.brand.primary,
			light: tokens.brand.primaryLight,
			dark: tokens.brand.primaryDark,
		},
		secondary: {
			main: tokens.brand.secondary,
			light: tokens.brand.secondaryLight,
			dark: tokens.brand.secondaryDark,
		},
		error: tokens.semantic.error,
		success: tokens.semantic.success,
		warning: tokens.semantic.warning,
		info: tokens.semantic.info,
		background: {
			default: tokens.background.default,
			paper: tokens.background.paper,
		},
		text: {
			primary: tokens.text.primary,
			secondary: tokens.text.secondary,
			disabled: tokens.text.disabled,
		},
		divider: tokens.border.default,
	},
	typography: {
		fontFamily: "'JetBrains Mono', 'Fira Code', 'SF Mono', monospace",
		h1: { fontSize: "2rem", fontWeight: 700, letterSpacing: "-0.02em" },
		h2: { fontSize: "1.5rem", fontWeight: 600, letterSpacing: "-0.01em" },
		h3: { fontSize: "1.25rem", fontWeight: 600 },
		h4: { fontSize: "1.1rem", fontWeight: 600 },
		body1: { fontSize: "0.875rem", lineHeight: 1.6 },
		body2: { fontSize: "0.8rem", lineHeight: 1.5 },
		caption: { fontSize: "0.7rem", letterSpacing: "0.04em", textTransform: "uppercase" as const },
	},
	shape: {
		borderRadius: tokens.borderRadius.md,
	},
	components: {
		MuiCssBaseline: {
			styleOverrides: {
				body: {
					backgroundColor: tokens.background.default,
					scrollbarWidth: "thin",
					scrollbarColor: `${tokens.border.subtle} ${tokens.background.default}`,
				},
			},
		},
		MuiPaper: {
			defaultProps: { elevation: 0 },
			styleOverrides: {
				root: {
					backgroundImage: "none",
					border: `1px solid ${tokens.border.default}`,
				},
			},
		},
		MuiCard: {
			defaultProps: { elevation: 0 },
			styleOverrides: {
				root: {
					border: `1px solid ${tokens.border.default}`,
					backgroundColor: tokens.background.paper,
				},
			},
		},
		MuiButton: {
			defaultProps: { disableElevation: true },
			styleOverrides: {
				root: { textTransform: "none" as const, fontWeight: 500 },
			},
		},
		MuiChip: {
			styleOverrides: {
				root: { fontWeight: 500, fontSize: "0.75rem" },
			},
		},
		MuiTooltip: {
			styleOverrides: {
				tooltip: {
					backgroundColor: tokens.background.elevated,
					border: `1px solid ${tokens.border.subtle}`,
					fontSize: "0.75rem",
				},
			},
		},
	},
});
