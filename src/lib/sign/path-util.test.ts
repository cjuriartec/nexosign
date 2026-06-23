import { describe, expect, it } from "vitest";
import { pdfBasenameFromPath } from "./path-util";

describe("pdfBasenameFromPath", () => {
	it("extrae nombre con barra invertida Windows", () => {
		expect(pdfBasenameFromPath("D:\\docs\\contrato.pdf")).toBe("contrato.pdf");
	});

	it("extrae nombre con barra POSIX", () => {
		expect(pdfBasenameFromPath("/home/user/informe.pdf")).toBe("informe.pdf");
	});

	it("devuelve la entrada si no hay separadores", () => {
		expect(pdfBasenameFromPath("solo.pdf")).toBe("solo.pdf");
	});

	it("ignora segmentos vacíos por barras finales", () => {
		expect(pdfBasenameFromPath("C:\\carpeta\\")).toBe("carpeta");
	});
});
