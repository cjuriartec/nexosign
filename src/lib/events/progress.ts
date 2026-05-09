import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ProgressPayload = {
	actual: number;
	total: number;
	job_id: string;
};

/** Suscripción al canal `progreso` emitido desde Rust (HTTP o comandos). */
export async function subscribeProgress(
	onEvent: (payload: ProgressPayload) => void,
): Promise<UnlistenFn> {
	return listen<ProgressPayload>("progreso", (event) => {
		onEvent(event.payload);
	});
}
