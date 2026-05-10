/** Quita `?intent=` de la barra de direcciones tras cargar la solicitud portal. */
export function stripIntentQueryFromBrowser(): void {
	if (typeof window === "undefined") return;
	const u = new URL(window.location.href);
	u.searchParams.delete("intent");
	history.replaceState({}, "", `${u.pathname}${u.search}${u.hash}`);
}
