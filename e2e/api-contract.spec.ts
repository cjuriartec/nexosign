import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { test, expect } from "@playwright/test";

const BASE = "http://127.0.0.1:14500";
const HEALTH_URL = `${BASE}/health`;

/**
 * Contrato HTTP real (servidor Axum dentro de la app Tauri).
 *
 * Requiere **dos terminales** cuando quieras ejecutar este caso:
 * 1) `npm run tauri dev` (deja la API escuchando en :14500)
 * 2) `NEXOSIGN_E2E_API=1 npm run test:e2e`
 *
 * Si solo pones `NEXOSIGN_E2E_API=1` sin Tauri, el test se omite con aviso
 * (no falla por ECONNREFUSED).
 */
test.describe("API local opcional", () => {
	test("GET /health devuelve nexosign", async ({ request }) => {
		test.skip(
			!process.env.NEXOSIGN_E2E_API,
			"Sin NEXOSIGN_E2E_API: este test no se ejecuta (smoke solo UI).",
		);

		let res: Awaited<ReturnType<typeof request.get>>;
		try {
			res = await request.get(HEALTH_URL, { timeout: 10_000 });
		} catch {
			test.skip(
				true,
				"No hay servidor en 127.0.0.1:14500. En **otra terminal** ejecuta primero: npm run tauri dev",
			);
			return;
		}

		expect(res.ok(), `HTTP ${res.status()}`).toBeTruthy();
		const body = await res.json();
		expect(body.service).toBe("nexosign");
		expect(body.status).toBe("ok");
	});

	test("POST /api/v1/batch/sign encola con job_id (ruta absoluta .pdf)", async ({
		request,
	}) => {
		test.skip(
			!process.env.NEXOSIGN_E2E_API,
			"Sin NEXOSIGN_E2E_API: este test no se ejecuta.",
		);

		const tmpPdf = path.join(
			os.tmpdir(),
			`nexosign-e2e-batch-${Date.now()}.pdf`,
		);
		fs.writeFileSync(tmpPdf, "%PDF-1.4\n");

		let res: Awaited<ReturnType<typeof request.post>>;
		try {
			res = await request.post(`${BASE}/api/v1/batch/sign`, {
				data: JSON.stringify({
					cert_id_hex: "00",
					inputs: [tmpPdf],
					job_id: "e2e-batch-contract",
				}),
				headers: { "Content-Type": "application/json" },
				timeout: 10_000,
			});
		} catch {
			test.skip(
				true,
				"No hay servidor en 127.0.0.1:14500. Ejecuta primero: npm run tauri dev",
			);
			return;
		} finally {
			try {
				fs.unlinkSync(tmpPdf);
			} catch {
				/* ignore */
			}
		}

		expect(res.ok(), `HTTP ${res.status()}`).toBeTruthy();
		const body = await res.json();
		expect(body.queued).toBe(true);
		expect(body.job_id).toBe("e2e-batch-contract");
	});
});
