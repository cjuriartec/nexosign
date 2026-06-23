import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("svelte-sonner", () => ({
	toast: {
		warning: vi.fn(),
	},
}));

import { toast } from "svelte-sonner";
import {
	humanizePdfRejectionReason,
	humanizeUserFacingError,
	toastPdfRejections,
} from "./pdf-rejection-toast";

const mockWarning = toast.warning as ReturnType<typeof vi.fn>;

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

	it("resume extensión inválida", () => {
		expect(humanizePdfRejectionReason("solo se admiten .pdf: C:\\x.txt")).toBe(
			"Extensión no válida",
		);
	});

	it("trunca motivos largos sin clasificar", () => {
		const long = "e".repeat(80);
		expect(humanizePdfRejectionReason(long)).toBe(`${"e".repeat(69)}…`);
	});
});

describe("humanizeUserFacingError", () => {
	it("resume errores PAdES de ByteRange", () => {
		expect(humanizeUserFacingError("hueco /ByteRange demasiado pequeño")).toBe(
			"PDF no compatible con firma incremental",
		);
	});

	it("resume trailer o MediaBox", () => {
		expect(
			humanizeUserFacingError("leer PDF (MediaBox): couldn't parse input: invalid file trailer"),
		).toBe("PDF dañado o no estándar");
	});

	it("resume fallos CMS genéricos", () => {
		expect(humanizeUserFacingError("PDF inválido u operación PAdES fallida: CMS")).toBe(
			"No se pudo completar la firma",
		);
	});
});

describe("toastPdfRejections", () => {
	afterEach(() => {
		vi.clearAllMocks();
	});

	it("no hace nada con lista vacía", () => {
		toastPdfRejections([]);
		expect(mockWarning).not.toHaveBeenCalled();
	});

	it("un archivo: nombre + motivo breve", () => {
		toastPdfRejections([
			{
				path: "D:\\docs\\CV.pdf",
				reason: "cabecera PDF incompleta (se espera %PDF)",
			},
		]);
		expect(mockWarning).toHaveBeenCalledWith("CV.pdf omitido", {
			description: "No es un PDF válido",
			duration: 5500,
		});
	});

	it("varios archivos: resumen agrupado sin rutas completas", () => {
		toastPdfRejections([
			{ path: "D:\\a\\one.pdf", reason: "cabecera PDF incompleta (se espera %PDF)" },
			{ path: "D:\\b\\two.pdf", reason: "cabecera PDF incompleta (se espera %PDF)" },
			{ path: "D:\\c\\three.pdf", reason: "demasiado grande (máx. 50 MiB por archivo)" },
		]);
		expect(mockWarning).toHaveBeenCalledWith(
			"3 archivos omitidos",
			expect.objectContaining({
				duration: 5500,
				description: expect.stringMatching(/^one\.pdf, two\.pdf · No es un PDF válido/),
			}),
		);
		const desc = mockWarning.mock.calls[0][1].description as string;
		expect(desc).not.toContain("D:\\");
	});
});
