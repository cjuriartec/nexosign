import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
	fetchHealth,
	fetchPing,
	postBatchSign,
	postBatchSignIntent,
	postBatchSignIntentFormData,
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

	it("postBatchSignIntent POSTea inputs y parsea request_id y deep_link", async () => {
		globalThis.fetch = vi.fn().mockResolvedValue({
			ok: true,
			json: async () => ({
				request_id: "550e8400-e29b-41d4-a716-446655440000",
				deep_link:
					"nexosign://sign?intent=550e8400-e29b-41d4-a716-446655440000",
			}),
		});
		const r = await postBatchSignIntent(
			{ inputs: ["/abs/doc.pdf"] },
			"http://mock.test",
		);
		expect(r.request_id).toBe("550e8400-e29b-41d4-a716-446655440000");
		expect(r.deep_link).toContain("nexosign://sign?intent=");
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
				deep_link: "nexosign://sign?intent=r1",
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
});