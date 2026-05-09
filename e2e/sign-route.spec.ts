import { test, expect } from "@playwright/test";

/**
 * UI estática (vite preview): sin Tauri, `?intent=` no dispara invoke;
 * solo comprobamos que la ruta es estable para deep links / recarga.
 */
test.describe("Ruta Firmar", () => {
	test("/sign?intent= muestra el asistente sin romper la vista", async ({
		page,
	}) => {
		await page.goto("/sign?intent=00000000-0000-0000-0000-000000000001");
		await expect(page).toHaveURL(/\/sign/);
		await expect(
			page.getByRole("heading", { name: /^Firmar$/ }),
		).toBeVisible();
	});
});
