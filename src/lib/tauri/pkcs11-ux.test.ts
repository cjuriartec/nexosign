import { describe, expect, it } from "vitest";
import { emptySigningCertsHelp } from "./pkcs11-ux";

describe("emptySigningCertsHelp", () => {
	it("sin slots indica ausencia de token", () => {
		const h = emptySigningCertsHelp(0);
		expect(h.title.toLowerCase()).toContain("detecta");
		expect(h.description.toLowerCase()).toMatch(/lector|middleware|nexosign_pkcs11_module/i);
	});

	it("con slots pero sin certs distingue perfil de firma", () => {
		const h = emptySigningCertsHelp(1);
		expect(h.title.toLowerCase()).toContain("sin certificado");
	});
});
