/**
 * UX helpers: polling de certificados, mensajes unificados sin certificados,
 * y persistir middleware preferido tras un lote exitoso (si el usuario no definió uno).
 */

import * as pkcs11 from "./pkcs11";
import { getPkcs11PreferredModule, setPkcs11PreferredModule } from "./settings";

/** Intervalo para refrescar certificados cuando el asistente está activo (plug-and-play ligero). */
export const PKCS11_CERT_POLL_MS = 3500;

export type EmptySigningCertsHelp = {
	title: string;
	description: string;
};

/** Mensaje único cuando no hay certificados de firma: distingue “sin token” vs “token sin cert de firma”. */
export function emptySigningCertsHelp(slotsWithToken: number): EmptySigningCertsHelp {
	if (slotsWithToken <= 0) {
		return {
			title: "No se detecta el token de firma",
			description:
				"Conecte el lector, inserte la tarjeta y espere unos segundos; la lista se actualiza sola mientras este paso está abierto. Si sigue igual, revise el middleware PKCS#11 en Ajustes o la variable NEXOSIGN_PKCS11_MODULE.",
		};
	}
	return {
		title: "Dispositivo detectado, sin certificado de firma",
		description:
			"El lector reconoce un token, pero no aparece un certificado de firma electrónica (solo autenticación u otro perfil). Seleccione el certificado de firma del DNIe o token en el chip.",
	};
}

/**
 * Si el usuario no guardó un middleware preferido, registra el módulo activo tras un lote sin errores
 * para acelerar el siguiente arranque (prioridad en driver.rs).
 */
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
