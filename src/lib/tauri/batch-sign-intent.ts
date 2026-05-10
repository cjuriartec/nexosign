import { invoke } from "@tauri-apps/api/core";

/** Respuesta de `get_batch_sign_intent` (serde camelCase). */
export type BatchSignIntentPayload = {
	inputs: string[];
	outputDir?: string | null;
};

/** Datos guardados por `POST /api/v1/batch/sign/intent` (solo proceso NexoSign). */
export async function getBatchSignIntent(
	requestId: string,
): Promise<BatchSignIntentPayload | null> {
	return invoke<BatchSignIntentPayload | null>("get_batch_sign_intent", {
		requestId,
	});
}

/** Fila para Colas — mismo origen que `POST …/batch/sign/intent`. */
export type PendingIntentRow = {
	requestId: string;
	fileCount: number;
	label: string;
	/** Segundos desde epoch (servidor). */
	createdAt: number;
};

export async function listPendingBatchIntents(): Promise<PendingIntentRow[]> {
	return invoke<PendingIntentRow[]>("list_pending_batch_intents");
}

export async function removePendingBatchIntent(requestId: string): Promise<void> {
	await invoke("remove_pending_batch_intent", { requestId });
}
