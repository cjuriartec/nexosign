import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { fetchHealth, fetchPing } from "./local-api";

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
});
