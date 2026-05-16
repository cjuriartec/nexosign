import { describe, expect, it } from "vitest";
import { emptySigningCertsHelp } from "./pkcs11-ux";

describe("emptySigningCertsHelp", () => {
	it("sin tarjeta indica ausencia de DNIe o tarjeta y no expone jerga PKCS#11", () => {
		const h = emptySigningCertsHelp(0);
		expect(h.title.toLowerCase()).toMatch(/dnie|tarjeta/);
		expect(h.description.toLowerCase()).toMatch(/lector|dnie|tarjeta/);
		const combined = `${h.title} ${h.description}`.toLowerCase();
		expect(combined).not.toMatch(/middleware|pkcs#?11|nexosign_pkcs11_module|token/);
	});

	it("con tarjeta presente pero sin certs indica que falta el certificado de firma", () => {
		const h = emptySigningCertsHelp(1);
		expect(h.title.toLowerCase()).toContain("sin certificado");
		expect(h.title.toLowerCase()).not.toContain("token");
	});
});
