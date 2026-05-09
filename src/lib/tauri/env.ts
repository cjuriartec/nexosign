/** True dentro del WebView de Tauri (no en navegador plano ni en Vitest). */
export function isTauriRuntime(): boolean {
	return typeof globalThis !== "undefined" && "__TAURI_INTERNALS__" in globalThis;
}
