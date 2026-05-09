import { test, expect } from "@playwright/test";

test.describe("UI NexoSign", () => {
	test("muestra título y tarjetas de fase 1", async ({ page }) => {
		await page.goto("/");
		await expect(
			page.getByRole("heading", { name: /NexoSign/i }),
		).toBeVisible();
		await expect(page.getByRole("heading", { name: /API local/i })).toBeVisible();
	});
});
