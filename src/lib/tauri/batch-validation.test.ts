import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
	invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { partitionBatchPdfPaths, validateBatchPdfPaths } from "./batch-validation";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

describe("batch-validation", () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it("validateBatchPdfPaths invoca el comando Tauri", async () => {
		mockInvoke.mockResolvedValueOnce(undefined);
		await validateBatchPdfPaths(["C:\\a.pdf"]);
		expect(mockInvoke).toHaveBeenCalledWith("validate_batch_pdf_paths", {
			paths: ["C:\\a.pdf"],
		});
	});

	it("partitionBatchPdfPaths devuelve aceptados y rechazados", async () => {
		const rejected = [{ path: "C:\\x.txt", reason: "solo se admiten .pdf" }];
		mockInvoke.mockResolvedValueOnce([["C:\\ok.pdf"], rejected]);
		const r = await partitionBatchPdfPaths(["C:\\ok.pdf", "C:\\x.txt"]);
		expect(r.accepted).toEqual(["C:\\ok.pdf"]);
		expect(r.rejected).toEqual(rejected);
		expect(mockInvoke).toHaveBeenCalledWith("partition_batch_pdf_paths", {
			paths: ["C:\\ok.pdf", "C:\\x.txt"],
		});
	});
});
