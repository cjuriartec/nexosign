import { redirect } from "@sveltejs/kit";
import type { PageLoad } from "./$types";

/** La app está pensada para firmar; la pantalla de inicio es Firmar. */
export const load: PageLoad = () => {
	throw redirect(302, "/sign");
};
