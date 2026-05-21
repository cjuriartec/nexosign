/**
 * UX helpers: mensajes sin certificados y persistir middleware preferido tras un lote exitoso.
 */

import * as pkcs11 from "./pkcs11";
import type {
	Pkcs11ProbeCertificateListing,
	SigningCertSource,
	SigningCertSummary,
} from "./pkcs11";
import { getPkcs11PreferredModule, setPkcs11PreferredModule } from "./settings";

export type EmptySigningCertsHelp = {
	title: string;
	description: string;
};

/** Etiqueta legible del origen del certificado (validación chip vs almacén Windows). */
export function signingCertSourceLabel(source?: SigningCertSource): string {
	if (source === "pkcs11") return "Lector (chip)";
	if (source === "win_my") return "Windows (MY)";
	return "—";
}

/** Mensaje único cuando no hay certificados de firma: distingue “sin tarjeta” vs “tarjeta sin certificado de firma”. */
export function emptySigningCertsHelp(slotsWithToken: number): EmptySigningCertsHelp {
	if (slotsWithToken <= 0) {
		return {
			title: "No se detecta el DNIe ni la tarjeta de firma",
			description:
				"Conecta el lector e inserta el DNIe (o tu tarjeta) y pulsa «Recargar». Si no aparece, revisa el lector en Ajustes o usa «Reinicializar lector».",
		};
	}
	return {
		title: "Lector conectado, pero sin certificado de firma",
		description:
			"El lector reconoce tu DNIe o tarjeta, pero no encontramos ningún certificado de firma en el chip (puede que solo haya certificados de autenticación). En Ajustes → «Lector de DNIe y tarjetas» comprueba el controlador PKCS#11 del fabricante o pulsa «Reinicializar lector» y vuelve aquí. No hace falta abrir otra aplicación para firmar.",
	};
}

/**
 * Si el usuario no guardó un middleware preferido, registra el módulo activo tras un lote sin errores
 * para acelerar el siguiente arranque (prioridad en driver.rs).
 */
export function hasPkcs11ChipCerts(certs: SigningCertSummary[]): boolean {
	return certs.some((c) => c.source === "pkcs11");
}

export function onlyWinMySigningCerts(certs: SigningCertSummary[]): boolean {
	return certs.length > 0 && certs.every((c) => c.source === "win_my");
}

/** Suma de slots con token en el probe. */
export function probeTotalSlotsWithToken(probe: Pkcs11ProbeCertificateListing | null): number {
	if (!probe) return 0;
	return probe.modules.reduce((n, m) => n + m.slots_with_token, 0);
}

/** Suma de certificados de firma detectados en chip (tras filtro) en el probe. */
export function probeTotalSigningOnChip(probe: Pkcs11ProbeCertificateListing | null): number {
	if (!probe) return 0;
	return probe.modules.reduce(
		(n, m) => n + m.slots.reduce((s, slot) => s + slot.signing_after_filter_count, 0),
		0,
	);
}

export function probeTotalRawOnChip(probe: Pkcs11ProbeCertificateListing | null): number {
	if (!probe) return 0;
	return probe.modules.reduce(
		(n, m) => n + m.slots.reduce((s, slot) => s + slot.raw_x509_count, 0),
		0,
	);
}

/** Texto de ayuda cuando solo aparece certificado Windows pero el lector responde. */
export function winMyOnlyChipUnreadableMessage(
	probe: Pkcs11ProbeCertificateListing | null,
	slotsWithTokenCount: number,
): string | null {
	const slots = Math.max(probeTotalSlotsWithToken(probe), slotsWithTokenCount);
	if (slots <= 0) return null;
	const raw = probeTotalRawOnChip(probe);
	const signing = probeTotalSigningOnChip(probe);
	if (raw === 0) {
		return "El lector está conectado, pero no vemos certificados en el chip sin PIN. Usa «Probar con PIN» en esta página o configura el controlador PKCS#11 del DNIe en Ajustes. El certificado «Windows (MY)» puede ser una copia en el PC, no el chip.";
	}
	if (raw > 0 && signing === 0) {
		return "Hay certificados en la tarjeta, pero ninguno tiene uso de firma electrónica (nonRepudiation) según nuestro criterio. Puede que solo haya certificados de autenticación en el chip.";
	}
	return null;
}

export async function maybePersistPreferredModuleAfterSuccessfulBatch(): Promise<void> {
	try {
		const existing = await getPkcs11PreferredModule();
		if (existing != null && existing.trim() !== "") {
			return;
		}
		const path = await pkcs11.probePkcs11ModulePath();
		const p = path?.trim();
		if (p) {
			await setPkcs11PreferredModule(p);
		}
	} catch {
		/* no bloquear la UI */
	}
}
