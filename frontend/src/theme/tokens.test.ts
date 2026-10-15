import { describe, expect, it } from "vitest";
import { tokens } from "./tokens";

describe("design tokens", () => {
	it("has valid hex color values for brand", () => {
		const hexRegex = /^#[0-9a-f]{6}$/i;
		expect(tokens.brand.primary).toMatch(hexRegex);
		expect(tokens.brand.secondary).toMatch(hexRegex);
		expect(tokens.brand.accent).toMatch(hexRegex);
		expect(tokens.brand.emerald).toMatch(hexRegex);
		expect(tokens.brand.rose).toMatch(hexRegex);
	});

	it("has valid hex color values for backgrounds", () => {
		const hexRegex = /^#[0-9a-f]{6}$/i;
		expect(tokens.background.default).toMatch(hexRegex);
		expect(tokens.background.paper).toMatch(hexRegex);
		expect(tokens.background.elevated).toMatch(hexRegex);
	});

	it("has consistent spacing scale", () => {
		expect(tokens.spacing.xs).toBeLessThan(tokens.spacing.sm);
		expect(tokens.spacing.sm).toBeLessThan(tokens.spacing.md);
		expect(tokens.spacing.md).toBeLessThan(tokens.spacing.lg);
		expect(tokens.spacing.lg).toBeLessThan(tokens.spacing.xl);
	});

	it("has increasing border radius values", () => {
		expect(tokens.borderRadius.sm).toBeLessThan(tokens.borderRadius.md);
		expect(tokens.borderRadius.md).toBeLessThan(tokens.borderRadius.lg);
	});

	it("has all semantic color groups", () => {
		for (const group of ["error", "success", "warning", "info"] as const) {
			expect(tokens.semantic[group]).toHaveProperty("main");
			expect(tokens.semantic[group]).toHaveProperty("light");
			expect(tokens.semantic[group]).toHaveProperty("dark");
		}
	});
});
