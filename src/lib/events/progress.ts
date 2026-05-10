import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ProgressPayload = {
	actual: number;
	total: number;
	/** Identificador del trabajo (Rust usa `job_id`). */
	job_id?: string;
	/** Alias camelCase por si el payload JSON llega transformado. */
	jobId?: string;
	nombre_archivo?: string;
	path?: string;
	/** Ruta absoluta del PDF firmado en disco si ese ítem se firmó correctamente. */
	output_path?: string | null;
	error?: string | null;
};

/** Suscripción al canal `progreso` emitido desde Rust (HTTP o comandos). */
export async function subscribeProgress(
	onEvent: (payload: ProgressPayload) => void,
): Promise<UnlistenFn> {
	return listen<ProgressPayload>("progreso", (event) => {
		onEvent(event.payload);
	});
}
