import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/tauri/env";

/** Misma forma que los ítems del store (evita dependencia circular). */
export type BatchQueueSnapshot = {
	items: Array<{
		jobId: string;
		status: string;
		label: string;
		progressPct: number;
		createdAt: number;
		finishedAt?: number;
	}>;
	activeBatchJobId: string | null;
	intentItems: Array<{
		requestId: string;
		label: string;
		fileCount: number;
		createdAt: number;
	}>;
	activeIntentRequestId: string | null;
};

/** Carga snapshot de colas desde SQLite (`allowed_origins.sqlite`, tablas `queue_*`). */
export async function backendLoadBatchQueueHistory(): Promise<BatchQueueSnapshot | null> {
	if (!isTauriRuntime()) return null;
	return invoke<BatchQueueSnapshot | null>("load_batch_queue_history");
}

/** Guarda snapshot (debounced desde el store). */
export async function backendSaveBatchQueueHistory(snapshot: BatchQueueSnapshot): Promise<void> {
	if (!isTauriRuntime()) return;
	await invoke("save_batch_queue_history", {
		payload: {
			items: snapshot.items.map((it) => ({
				jobId: it.jobId,
				status: it.status,
				label: it.label,
				progressPct: it.progressPct,
				createdAt: it.createdAt,
				finishedAt: it.finishedAt ?? null,
			})),
			activeBatchJobId: snapshot.activeBatchJobId,
			intentItems: snapshot.intentItems.map((it) => ({
				requestId: it.requestId,
				label: it.label,
				fileCount: it.fileCount,
				createdAt: it.createdAt,
			})),
			activeIntentRequestId: snapshot.activeIntentRequestId,
		},
	});
}
