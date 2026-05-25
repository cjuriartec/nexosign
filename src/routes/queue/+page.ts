import { redirect } from "@sveltejs/kit";

/** La vista de colas se eliminó; las solicitudes del portal abren Firmar. */
export function load({ url }: { url: URL }) {
	const intent = url.searchParams.get("intent");
	if (intent) {
		throw redirect(302, `/sign?intent=${encodeURIComponent(intent)}`);
	}
	throw redirect(302, "/sign");
}
