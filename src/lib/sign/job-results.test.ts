import { describe, expect, it } from "vitest";
import {
	buildSignJobFileDisplayList,
	resolveBatchOutputDirectoryHint,
	upsertJobFileResult,
} from "./job-results";

describe("upsertJobFileResult", () => {
	it("oculta MY duplicado por huella al actualizar éxito", () => {
		const out = upsertJobFileResult([], {
			index: 1,
			label: "a.pdf",
			inputPath: "/in/a.pdf",
			outputPath: "/out/a_firmado.pdf",
			error: null,
		});
		expect(out).toHaveLength(1);
		expect(out[0].outputPath).toBe("/out/a_firmado.pdf");
	});

	it("registra error sin output", () => {
		const out = upsertJobFileResult(
			[{ index: 1, label: "a.pdf", outputPath: "/out/a_firmado.pdf" }],
			{
				index: 1,
				label: "a.pdf",
				error: "fallo",
				outputPath: null,
			},
		);
		expect(out[0].error).toBe("fallo");
		expect(out[0].outputPath).toBeUndefined();
	});
});

describe("buildSignJobFileDisplayList", () => {
	it("marca pendiente mientras signing", () => {
		const list = buildSignJobFileDisplayList(["/a.pdf", "/b.pdf"], [], { signing: true });
		expect(list[0].status).toBe("pending");
		expect(list[1].status).toBe("pending");
	});

	it("marca ok cuando hay output", () => {
		const list = buildSignJobFileDisplayList(
			["/a.pdf"],
			[{ index: 1, label: "a.pdf", outputPath: "/out/a_firmado.pdf" }],
			{ signing: false },
		);
		expect(list[0].status).toBe("ok");
	});
});

describe("resolveBatchOutputDirectoryHint", () => {
	it("prioriza outputDirForJob", () => {
		const h = resolveBatchOutputDirectoryHint("/salida_firmados", []);
		expect(h.dir).toBe("/salida_firmados");
	});

	it("devuelve último output si no hay carpeta de lote", () => {
		const h = resolveBatchOutputDirectoryHint(null, [
			{ index: 1, label: "a.pdf", outputPath: "/x/a_firmado.pdf" },
		]);
		expect(h.lastOutputPath).toBe("/x/a_firmado.pdf");
	});
});
