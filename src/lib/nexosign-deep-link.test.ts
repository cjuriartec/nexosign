import { describe, it, expect } from "vitest";
import { extractIntentFromNexosignUrl } from "./nexosign-deep-link";

describe("extractIntentFromNexosignUrl", () => {
	it("lee intent desde nexosign://sign?intent=uuid", () => {
		expect(
			extractIntentFromNexosignUrl(
				"nexosign://sign?intent=550e8400-e29b-41d4-a716-446655440000",
			),
		).toBe("550e8400-e29b-41d4-a716-446655440000");
	});

	it("devuelve null si no es protocolo nexosign", () => {
		expect(extractIntentFromNexosignUrl("https://example.com/sign?intent=x")).toBeNull();
	});

	it("devuelve null si falta intent", () => {
		expect(extractIntentFromNexosignUrl("nexosign://sign")).toBeNull();
	});

	it("devuelve null si la URL es inválida", () => {
		expect(extractIntentFromNexosignUrl(":::no-es-url")).toBeNull();
	});
});
