import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
	fetchHealth,
	fetchPing,
	fetchBatchJobStatus,
	postBatchSign,
	postBatchSignIntent,
	postBatchSignIntentFormData,
	fetchBatchSignIntentStatus,
	fetchBatchSignedManifest,
	fetchBatchSignedPdfBlob,
	batchSignedFileAbsoluteUrl,
	extractJsonErrorMessage,
} from "./local-api";

describe("local-api", () => {
	const originalFetch = globalThis.fetch;

	beforeEach(() => {
		vi.resetAllMocks();
	});

	afterEach(() => {
		globalThis.fetch = originalFetch;
	});

	it("fetchHealth parsea respuesta ok", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({
				status: "ok",
				service: "nexosign",
				version: "0.1.0",
			}),
		});

		const h = await fetchHealth("http://mock.test");
		expect(h.service).toBe("nexosign");
		expect(globalThis.fetch).toHaveBeenCalledWith("http://mock.test/health");
	});

	it("fetchHealth lanza si no ok", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({ ok: false, status: 500 });
		await expect(fetchHealth("http://mock.test")).rejects.toThrow("health failed");
	});

	it("fetchPing envía POST JSON", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({ ok: true }),
		});
		const p = await fetchPing("http://mock.test");
		expect(p.ok).toBe(true);
		expect(globalThis.fetch).toHaveBeenCalledWith(
			"http://mock.test/api/v1/ping",
			expect.objectContaining({ method: "POST" }),
		);
	});

	it("postBatchSign envía POST JSON y parsea job_id", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({
				job_id: "job-1",
				queued: true,
			}),
		});
		const b = await postBatchSign(
			{
				cert_id_hex: "ab",
				inputs: ["/tmp/a.pdf"],
				job_id: "job-1",
			},
			"http://mock.test",
		);
		expect(b.queued).toBe(true);
		expect(b.job_id).toBe("job-1");
		expect(globalThis.fetch).toHaveBeenCalledWith(
			"http://mock.test/api/v1/batch/sign",
			expect.objectContaining({
				method: "POST",
				body: JSON.stringify({
					cert_id_hex: "ab",
					inputs: ["/tmp/a.pdf"],
					job_id: "job-1",
				}),
			}),
		);
	});

	it("postBatchSign incluye intent_request_id en el JSON", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({ job_id: "j2", queued: true }),
		});
		await postBatchSign(
			{
				cert_id_hex: "cd",
				inputs: ["/tmp/b.pdf"],
				intent_request_id: "req-intent-99",
			},
			"http://mock.test",
		);
		expect(globalThis.fetch).toHaveBeenCalledWith(
			"http://mock.test/api/v1/batch/sign",
			expect.objectContaining({
				body: JSON.stringify({
					cert_id_hex: "cd",
					inputs: ["/tmp/b.pdf"],
					intent_request_id: "req-intent-99",
				}),
			}),
		);
	});

	it("postBatchSignIntent POSTea inputs y parsea request_id", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({
				request_id: "550e8400-e29b-41d4-a716-446655440000",
			}),
		});
		const r = await postBatchSignIntent(
			{ inputs: ["/abs/doc.pdf"] },
			"http://mock.test",
		);
		expect(r.request_id).toBe("550e8400-e29b-41d4-a716-446655440000");
		expect(globalThis.fetch).toHaveBeenCalledWith(
			"http://mock.test/api/v1/batch/sign/intent",
			expect.objectContaining({
				method: "POST",
				body: JSON.stringify({ inputs: ["/abs/doc.pdf"] }),
			}),
		);
	});

	it("postBatchSignIntentFormData no envía Content-Type (FormData) y parsea", async () => {
		const fd = new FormData();
		const blob = new Blob(["%PDF-1.4\n"], { type: "application/pdf" });
		fd.append("files", blob, "a.pdf");
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({
				request_id: "r1",
			}),
		});
		const r = await postBatchSignIntentFormData(fd, "http://mock.test");
		expect(r.request_id).toBe("r1");
		const call = (globalThis.fetch as ReturnType<typeof vi.fn>).mock
			.calls[0] as [string, RequestInit];
		const init = call[1];
		expect(init.method).toBe("POST");
		expect(init.body).toBe(fd);
		const h = init.headers as Record<string, string>;
		expect(h["Content-Type"]).toBeUndefined();
		expect(h["Origin"]).toBe("http://localhost:1420");
	});

	it("postBatchSign añade Origin en entorno Node", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({ job_id: "j3", queued: true }),
		});
		await postBatchSign(
			{
				cert_id_hex: "ef",
				inputs: ["/tmp/c.pdf"],
			},
			"http://mock.test",
		);
		const call = (globalThis.fetch as ReturnType<typeof vi.fn>).mock
			.calls[0] as [string, RequestInit];
		const h = call[1].headers as Record<string, string>;
		expect(h["Origin"]).toBe("http://localhost:1420");
		expect(h["Content-Type"]).toBe("application/json");
	});

	it("postBatchSignIntentFormData mapea error HTTP con mensaje JSON", async () => {
		const fd = new FormData();
		fd.append("files", new Blob(["bad"], { type: "application/pdf" }), "bad.pdf");
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: false,
			status: 400,
			json: async () => ({ error: "no es un PDF válido" }),
		});
		await expect(
			postBatchSignIntentFormData(fd, "http://mock.test"),
		).rejects.toThrow(/no es un PDF válido/);
	});

	it("fetchBatchJobStatus GETea estado y envía Origin en Node", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({
				job_id: "jid-1",
				phase: "running",
				actual: 1,
				total: 3,
			}),
		});
		const s = await fetchBatchJobStatus("jid-1", "http://mock.test");
		expect(s.phase).toBe("running");
		expect(globalThis.fetch).toHaveBeenCalledWith(
			"http://mock.test/api/v1/batch/jobs/jid-1/status",
			expect.objectContaining({
				headers: expect.objectContaining({
					Origin: "http://localhost:1420",
				}),
			}),
		);
	});

	it("postBatchSignIntent lanza si no ok", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: false,
			status: 400,
			json: async () => ({ error: "bad" }),
		});
		await expect(
			postBatchSignIntent({ inputs: ["/x.pdf"] }, "http://mock.test"),
		).rejects.toThrow(/bad/);
	});

	it("fetchBatchSignIntentStatus GETea URL codificada", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({
				request_id: "rid",
				phase: "processing",
				job_id: "j1",
			}),
		});
		const s = await fetchBatchSignIntentStatus("id/with slash", "http://mock.test");
		expect(s.phase).toBe("processing");
		expect(globalThis.fetch).toHaveBeenCalledWith(
			"http://mock.test/api/v1/batch/sign/intent/id%2Fwith%20slash/status",
			expect.any(Object),
		);
	});

	it("fetchBatchSignedManifest parsea lista de ficheros", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({
				job_id: "jb",
				count: 1,
				files: [{ index: 0, filename: "a.pdf", href: "/api/..." }],
			}),
		});
		const m = await fetchBatchSignedManifest("jb", "http://mock.test");
		expect(m.count).toBe(1);
		expect(m.files[0].filename).toBe("a.pdf");
	});

	it("fetchBatchSignedPdfBlob devuelve Blob", async () => {
		const blob = new Blob(["%PDF"], { type: "application/pdf" });
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			blob: async () => blob,
		});
		const out = await fetchBatchSignedPdfBlob("j", 0, "http://mock.test");
		expect(out).toBe(blob);
		expect(globalThis.fetch).toHaveBeenCalledWith(
			"http://mock.test/api/v1/batch/jobs/j/files/0",
			expect.any(Object),
		);
	});

	it("batchSignedFileAbsoluteUrl construye ruta con índice", () => {
		expect(batchSignedFileAbsoluteUrl("ab/cd", 2, "http://x")).toBe(
			"http://x/api/v1/batch/jobs/ab%2Fcd/files/2",
		);
	});

	it("extractJsonErrorMessage prioriza detail sobre error", () => {
		expect(extractJsonErrorMessage({ detail: "d", error: "e" })).toBe("d");
		expect(extractJsonErrorMessage({ error: "solo" })).toBe("solo");
		expect(extractJsonErrorMessage(null)).toBeUndefined();
	});
});