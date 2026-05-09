import { describe, it, expect, vi, beforeEach } from "vitest";
import { getBatchSignIntent } from "./batch-sign-intent";

vi.mock("@tauri-apps/api/core", () => ({
	invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

describe("getBatchSignIntent", () => {
	beforeEach(() => {
		vi.mocked(invoke).mockReset();
	});

	it("llama get_batch_sign_intent con requestId", async () => {
		vi.mocked(invoke).mockResolvedValue({
			inputs: ["/a.pdf"],
			outputDir: "/out",
		});
		const r = await getBatchSignIntent("uuid-1");
		expect(r?.inputs).toEqual(["/a.pdf"]);
		expect(r?.outputDir).toBe("/out");
		expect(invoke).toHaveBeenCalledWith("get_batch_sign_intent", {
			requestId: "uuid-1",
		});
	});

	it("propaga null si no hay intención", async () => {
		vi.mocked(invoke).mockResolvedValue(null);
		const r = await getBatchSignIntent("missing");
		expect(r).toBeNull();
	});
});
