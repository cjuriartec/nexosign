import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
	invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { ipcFetchHealth, ipcPostBatchSign, LocalBackendInvokeError } from "./local-backend";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

describe("local-backend", () => {
	afterEach(() => {
		vi.clearAllMocks();
	});

	it("ipcFetchHealth devuelve el payload de invoke", async () => {
		mockInvoke.mockResolvedValueOnce({ status: "ok", service: "nexosign", version: "0.1.0" });
		const h = await ipcFetchHealth();
		expect(h.status).toBe("ok");
		expect(mockInvoke).toHaveBeenCalledWith("local_api_health");
	});

	it("ipcPostBatchSign normaliza jobId camelCase", async () => {
		mockInvoke.mockResolvedValueOnce({ jobId: "j1", queued: true });
		const r = await ipcPostBatchSign({
			cert_id_hex: "ab",
			inputs: ["/a.pdf"],
			pin: "x",
		});
		expect(r).toEqual({ job_id: "j1", queued: true });
	});

	it("errores invoke con code/detail lanzan LocalBackendInvokeError", async () => {
		mockInvoke.mockRejectedValueOnce({ code: "bad_request", detail: "PIN vacío" });
		await expect(
			ipcPostBatchSign({ cert_id_hex: "ab", inputs: [], pin: "" }),
		).rejects.toMatchObject({
			name: "LocalBackendInvokeError",
			code: "bad_request",
			detail: "PIN vacío",
		});
	});
});
