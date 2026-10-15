/** Design tokens — mathematical cosmos aesthetic.
 *
 * Deep indigo/navy backgrounds, teal/cyan accents, amber/gold highlights.
 * Evokes an observatory at night where mathematical patterns emerge from data.
 */

export const tokens = {
	brand: {
		primary: "#06b6d4", // Cyan — main accent (epoch flow, active states)
		primaryLight: "#22d3ee",
		primaryDark: "#0891b2",
		secondary: "#f59e0b", // Amber — highlights (discoveries, rewards)
		secondaryLight: "#fbbf24",
		secondaryDark: "#d97706",
		accent: "#8b5cf6", // Violet — library/symbols
		accentLight: "#a78bfa",
		accentDark: "#7c3aed",
		emerald: "#10b981", // Success / compression gains
		rose: "#f43f5e", // Alert / novelty spikes
	},

	background: {
		default: "#0a0f1a", // Deep space navy
		paper: "#0f1729", // Card surfaces
		elevated: "#141e33", // Elevated elements
		subtle: "#1a2744", // Subtle backgrounds
		overlay: "rgba(10, 15, 26, 0.8)",
	},

	text: {
		primary: "#e2e8f0", // Slate 200
		secondary: "#94a3b8", // Slate 400
		disabled: "#475569", // Slate 600
		accent: "#06b6d4", // Cyan for links/interactive
	},

	border: {
		default: "#1e293b", // Slate 800
		subtle: "#334155", // Slate 700
		focus: "#06b6d4",
	},

	semantic: {
		error: { main: "#ef4444", light: "#f87171", dark: "#dc2626" },
		success: { main: "#10b981", light: "#34d399", dark: "#059669" },
		warning: { main: "#f59e0b", light: "#fbbf24", dark: "#d97706" },
		info: { main: "#3b82f6", light: "#60a5fa", dark: "#2563eb" },
	},

	spacing: {
		xs: 4,
		sm: 8,
		md: 16,
		lg: 24,
		xl: 32,
		"2xl": 48,
	},

	borderRadius: {
		sm: 4,
		md: 8,
		lg: 12,
		xl: 16,
	},
} as const;
