import { test, expect } from "@playwright/test";

test.describe("UI NexoSign", () => {
	test("muestra panel y navegación principal", async ({ page }) => {
		await page.goto("/");
		await expect(page.getByRole("heading", { name: /Panel/i })).toBeVisible();
		await expect(page.getByRole("link", { name: /Firmar PDFs/i })).toBeVisible();
		await expect(page.getByRole("link", { name: /Certificados/i })).toBeVisible();
	});
});
