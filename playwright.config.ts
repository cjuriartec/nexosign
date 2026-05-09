import { defineConfig, devices } from "@playwright/test";

/**
 * E2E UI: arranca `vite preview` en 4173 (solo frontend estático).
 * Para validar también GET /health contra la API en :14500, ejecuta con
 * `NEXOSIGN_E2E_API=1` teniendo el backend en marcha (`npm run tauri dev`).
 */
export default defineConfig({
	testDir: "e2e",
	fullyParallel: true,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 2 : 0,
	use: {
		baseURL: "http://localhost:4173",
		trace: "on-first-retry",
	},
	projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
	webServer: {
		command: "npm run build && npm run preview -- --port 4173 --strictPort",
		url: "http://localhost:4173",
		reuseExistingServer: !process.env.CI,
		timeout: 120_000,
	},
});
