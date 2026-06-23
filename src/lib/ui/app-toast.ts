import { toast } from "svelte-sonner";

const DEFAULT_MS = 4_500;
const ERROR_MS = 6_000;

/** Mensaje informativo breve (esquina, sin bloquear). */
export function toastInfo(message: string, description?: string) {
	toast.message(message, {
		duration: DEFAULT_MS,
		...(description ? { description } : {}),
	});
}

/** Aviso recuperable (validación, archivos omitidos, etc.). */
export function toastWarn(message: string, description?: string) {
	toast.warning(message, {
		duration: DEFAULT_MS,
		...(description ? { description } : {}),
	});
}

/** Error puntual: una línea, sin pared de texto. */
export function toastFail(message: string, description?: string) {
	const short = message.length > 120 ? `${message.slice(0, 117)}…` : message;
	toast.error(short, {
		duration: ERROR_MS,
		...(description && description.length <= 100 ? { description } : {}),
	});
}
