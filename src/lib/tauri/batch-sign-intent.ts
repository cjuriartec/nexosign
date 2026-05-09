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
