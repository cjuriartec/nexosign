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

/** Mensaje único cuando no hay certificados de firma: distingue “sin tarjeta” vs “tarjeta sin certificado de firma”. */
export function emptySigningCertsHelp(slotsWithToken: number): EmptySigningCertsHelp {
	if (slotsWithToken <= 0) {
		return {
			title: "No se detecta el DNIe ni la tarjeta de firma",
			description:
				"Conecta el lector e inserta el DNIe (o tu tarjeta). Espera unos segundos: la lista se actualiza sola mientras estás en esta pantalla. Si no aparece, revisa que el lector esté bien conectado o abre Ajustes para comprobar el controlador del lector.",
		};
	}
	return {
		title: "Lector conectado, pero sin certificado de firma",
		description:
			"El lector reconoce tu DNIe o tarjeta, pero no encontramos ningún certificado de firma electrónica (puede que solo haya certificados de autenticación). Comprueba que has insertado la tarjeta correcta y vuelve a intentarlo.",
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
