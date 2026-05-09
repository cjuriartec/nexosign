import { test, expect } from "@playwright/test";

test.describe("UI NexoSign", () => {
	test("la raíz abre la vista de firma y la navegación principal es visible", async ({
		page,
	}) => {
		await page.goto("/");
		await expect(page).toHaveURL(/\/sign$/);
		await expect(page.getByRole("heading", { name: /^Firmar$/ })).toBeVisible();
		await expect(page.getByRole("link", { name: /^Firmar$/ })).toBeVisible();
		await expect(page.getByRole("link", { name: /Certificados/i })).toBeVisible();
	});
});
