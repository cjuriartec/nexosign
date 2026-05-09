/** Prefijo que envía `TokenError::NoSlot` en Rust (ver `adapters/pkcs11/error.rs`). */
export const PKCS11_NO_TOKEN_PREFIX = "PKCS11_NO_TOKEN:";

/** Hay PKCS#11 pero no aparece ningún slot con tarjeta/token presente. */
export function isPkcs11NoTokenError(err: unknown): boolean {
	const s = String(err);
	return s.includes(PKCS11_NO_TOKEN_PREFIX) || s.includes("no hay slots con token");
}
