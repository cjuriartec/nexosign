/**
 * Extrae el identificador `intent` de URLs `nexosign://…` (deep link al asistente de firma).
 */
export function extractIntentFromNexosignUrl(urlStr: string): string | null {
	try {
		const u = new URL(urlStr);
		if (u.protocol !== "nexosign:") return null;
		return u.searchParams.get("intent");
	} catch {
		return null;
	}
}
