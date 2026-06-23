import { describe, expect, it } from "vitest";
import { humanizePdfRejectionReason } from "./pdf-rejection-toast";

describe("humanizePdfRejectionReason", () => {
	it("quita la ruta del motivo", () => {
		expect(
			humanizePdfRejectionReason(
				"D:\\proyecto\\fitac.pdf: cabecera PDF incompleta (se espera %PDF)",
			),
		).toBe("No es un PDF válido");
	});

	it("resume tamaño", () => {
		expect(humanizePdfRejectionReason("demasiado grande (máx. 50 MiB por archivo)")).toBe(
			"Supera 50 MiB",
		);
	});
});
