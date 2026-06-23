import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/tauri/batch", () => ({
	enumeratePdfsUnderFolder: vi.fn(),
}));

import { enumeratePdfsUnderFolder } from "$lib/tauri/batch";
import { ingestDroppedPaths } from "./ingest-dropped-paths";

const mockEnumerate = enumeratePdfsUnderFolder as ReturnType<typeof vi.fn>;

describe("ingestDroppedPaths", () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it("devuelve vacío sin rutas", async () => {
		const r = await ingestDroppedPaths([], async () => "/out");
		expect(r).toEqual({
			pdfs: [],
			sourceMode: null,
			folderPath: null,
			outputDirForJob: null,
		});
	});

	it("acepta PDF sueltos como modo files", async () => {
		mockEnumerate.mockRejectedValue(new Error("no es un directorio"));
		const r = await ingestDroppedPaths(
			["C:\\a.pdf", "C:\\b.PDF"],
			async () => "/out",
		);
		expect(r.sourceMode).toBe("files");
		expect(r.pdfs).toEqual(["C:\\a.pdf", "C:\\b.PDF"]);
		expect(r.folderPath).toBeNull();
	});

	it("una sola carpeta activa modo folder y outputDirForJob", async () => {
		mockEnumerate.mockResolvedValue(["C:\\lote\\a.pdf", "C:\\lote\\b.pdf"]);
		const r = await ingestDroppedPaths(
			["C:\\lote"],
			async (folder) => `${folder}_firmados`,
		);
		expect(r.sourceMode).toBe("folder");
		expect(r.folderPath).toBe("C:\\lote");
		expect(r.outputDirForJob).toBe("C:\\lote_firmados");
		expect(r.pdfs).toEqual(["C:\\lote\\a.pdf", "C:\\lote\\b.pdf"]);
	});

	it("mezcla carpeta y archivos como files", async () => {
		mockEnumerate.mockImplementation(async (p: string) => {
			if (p === "C:\\lote") return ["C:\\lote\\x.pdf"];
			throw new Error("no es un directorio");
		});
		const r = await ingestDroppedPaths(
			["C:\\lote", "C:\\sueltos\\y.pdf"],
			async () => "/out",
		);
		expect(r.sourceMode).toBe("files");
		expect(r.pdfs).toContain("C:\\lote\\x.pdf");
		expect(r.pdfs).toContain("C:\\sueltos\\y.pdf");
	});

	it("deduplica rutas de entrada", async () => {
		mockEnumerate.mockRejectedValue(new Error("no es un directorio"));
		const r = await ingestDroppedPaths(
			["C:\\a.pdf", "C:\\a.pdf"],
			async () => "/out",
		);
		expect(r.pdfs).toEqual(["C:\\a.pdf"]);
	});
});
