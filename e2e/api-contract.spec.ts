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
				headers: {
					"Content-Type": "application/json",
					Origin: "http://localhost:1420",
				},
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

	test("POST /api/v1/batch/sign/intent devuelve request_id (sin encolar)", async ({
		request,
	}) => {
		test.skip(
			!process.env.NEXOSIGN_E2E_API,
			"Sin NEXOSIGN_E2E_API: este test no se ejecuta.",
		);

		const tmpPdf = path.join(
			os.tmpdir(),
			`nexosign-e2e-intent-${Date.now()}.pdf`,
		);
		fs.writeFileSync(tmpPdf, "%PDF-1.4\n");

		let res: Awaited<ReturnType<typeof request.post>>;
		try {
			res = await request.post(`${BASE}/api/v1/batch/sign/intent`, {
				data: JSON.stringify({
					inputs: [tmpPdf],
				}),
				headers: {
					"Content-Type": "application/json",
					Origin: "http://localhost:1420",
				},
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
		expect(body.request_id).toBeTruthy();
		expect(body.deep_link).toBeUndefined();
	});

	test("POST /api/v1/batch/sign/intent multipart (archivo en memoria)", async ({
		request,
	}) => {
		test.skip(
			!process.env.NEXOSIGN_E2E_API,
			"Sin NEXOSIGN_E2E_API: este test no se ejecuta.",
		);

		let res: Awaited<ReturnType<typeof request.post>>;
		try {
			res = await request.post(`${BASE}/api/v1/batch/sign/intent`, {
				multipart: {
					files: {
						name: "e2e-multipart.pdf",
						mimeType: "application/pdf",
						buffer: Buffer.from("%PDF-1.4\n"),
					},
				},
				headers: {
					Origin: "http://localhost:1420",
				},
				timeout: 10_000,
			});
		} catch {
			test.skip(
				true,
				"No hay servidor en 127.0.0.1:14500. Ejecuta primero: npm run tauri dev",
			);
			return;
		}

		expect(res.ok(), `HTTP ${res.status()}`).toBeTruthy();
		const body = await res.json();
		expect(body.request_id).toBeTruthy();
		expect(body.deep_link).toBeUndefined();
	});

	test("POST /api/v1/batch/sign/intent sin Origin devuelve 403", async ({
		request,
	}) => {
		test.skip(
			!process.env.NEXOSIGN_E2E_API,
			"Sin NEXOSIGN_E2E_API: este test no se ejecuta.",
		);

		const tmpPdf = path.join(
			os.tmpdir(),
			`nexosign-e2e-intent-no-origin-${Date.now()}.pdf`,
		);
		fs.writeFileSync(tmpPdf, "%PDF-1.4\n");

		let res: Awaited<ReturnType<typeof request.post>>;
		try {
			res = await request.post(`${BASE}/api/v1/batch/sign/intent`, {
				data: JSON.stringify({ inputs: [tmpPdf] }),
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

		expect(res.status()).toBe(403);
		const body = await res.json();
		expect(String(body.error)).toContain("missing_origin");
	});

	test("POST /api/v1/batch/sign/intent con content-type inválido devuelve 415", async ({
		request,
	}) => {
		test.skip(
			!process.env.NEXOSIGN_E2E_API,
			"Sin NEXOSIGN_E2E_API: este test no se ejecuta.",
		);

		let res: Awaited<ReturnType<typeof request.post>>;
		try {
			res = await request.post(`${BASE}/api/v1/batch/sign/intent`, {
				data: "plain-text",
				headers: {
					"Content-Type": "text/plain",
					Origin: "http://localhost:1420",
				},
				timeout: 10_000,
			});
		} catch {
			test.skip(
				true,
				"No hay servidor en 127.0.0.1:14500. Ejecuta primero: npm run tauri dev",
			);
			return;
		}

		expect(res.status()).toBe(415);
	});

	test("POST /api/v1/batch/sign/intent multipart sin archivo devuelve 400", async ({
		request,
	}) => {
		test.skip(
			!process.env.NEXOSIGN_E2E_API,
			"Sin NEXOSIGN_E2E_API: este test no se ejecuta.",
		);

		let res: Awaited<ReturnType<typeof request.post>>;
		try {
			res = await request.post(`${BASE}/api/v1/batch/sign/intent`, {
				multipart: { output_dir: "/tmp" },
				headers: { Origin: "http://localhost:1420" },
				timeout: 10_000,
			});
		} catch {
			test.skip(
				true,
				"No hay servidor en 127.0.0.1:14500. Ejecuta primero: npm run tauri dev",
			);
			return;
		}

		expect(res.status()).toBe(400);
		const body = await res.json();
		expect(String(body.error)).toContain("no se recibió ningún PDF");
	});

	test("POST intent y luego batch con intent_request_id encola el trabajo", async ({
		request,
	}) => {
		test.skip(
			!process.env.NEXOSIGN_E2E_API,
			"Sin NEXOSIGN_E2E_API: este test no se ejecuta.",
		);

		const tmpPdf = path.join(
			os.tmpdir(),
			`nexosign-e2e-intent-flow-${Date.now()}.pdf`,
		);
		fs.writeFileSync(tmpPdf, "%PDF-1.4\n");

		let intentRes: Awaited<ReturnType<typeof request.post>>;
		try {
			intentRes = await request.post(`${BASE}/api/v1/batch/sign/intent`, {
				data: JSON.stringify({ inputs: [tmpPdf] }),
				headers: {
					"Content-Type": "application/json",
					Origin: "http://localhost:1420",
				},
				timeout: 10_000,
			});
		} catch {
			test.skip(
				true,
				"No hay servidor en 127.0.0.1:14500. Ejecuta primero: npm run tauri dev",
			);
			return;
		}

		expect(intentRes.ok(), `intent HTTP ${intentRes.status()}`).toBeTruthy();
		const intentBody = await intentRes.json();
		const rid = intentBody.request_id as string;

		let batchRes: Awaited<ReturnType<typeof request.post>>;
		try {
			batchRes = await request.post(`${BASE}/api/v1/batch/sign`, {
				data: JSON.stringify({
					cert_id_hex: "00",
					inputs: [tmpPdf],
					job_id: "e2e-after-intent",
					intent_request_id: rid,
				}),
				headers: {
					"Content-Type": "application/json",
					Origin: "http://localhost:1420",
				},
				timeout: 10_000,
			});
		} finally {
			try {
				fs.unlinkSync(tmpPdf);
			} catch {
				/* ignore */
			}
		}

		expect(batchRes.ok(), `batch HTTP ${batchRes.status()}`).toBeTruthy();
		const batchBody = await batchRes.json();
		expect(batchBody.queued).toBe(true);
		expect(batchBody.job_id).toBe("e2e-after-intent");
	});
});
