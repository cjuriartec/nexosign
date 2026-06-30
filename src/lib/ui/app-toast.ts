import { toast } from "$lib/ui/notify";

/** Mensaje informativo breve (notificación del sistema). */
export function toastInfo(message: string, description?: string) {
	toast.message(message, description ? { description } : undefined);
}

/** Aviso recuperable (validación, archivos omitidos, etc.). */
export function toastWarn(message: string, description?: string) {
	toast.warning(message, description ? { description } : undefined);
}

/** Error puntual: una línea, sin pared de texto. */
export function toastFail(message: string, description?: string) {
	const short = message.length > 120 ? `${message.slice(0, 117)}…` : message;
	const desc =
		description && description.length <= 100 ? description : undefined;
	toast.error(short, desc ? { description: desc } : undefined);
}
