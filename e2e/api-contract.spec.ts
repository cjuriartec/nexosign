import { test, expect } from "@playwright/test";

/**
 * Contrato HTTP real (servidor Axum en la app Tauri).
 * No forma parte del smoke por defecto: activar con NEXOSIGN_E2E_API=1
 * mientras corre `npm run tauri dev` o un binario que escuche en :14500.
 */
test.describe("API local opcional", () => {
	test("GET /health devuelve nexosign", async ({ request }) => {
		test.skip(
			!process.env.NEXOSIGN_E2E_API,
			"Definir NEXOSIGN_E2E_API=1 con la API escuchando en 14500.",
		);

		const res = await request.get("http://127.0.0.1:14500/health");
		expect(res.ok()).toBeTruthy();
		const body = await res.json();
		expect(body.service).toBe("nexosign");
		expect(body.status).toBe("ok");
	});
});
