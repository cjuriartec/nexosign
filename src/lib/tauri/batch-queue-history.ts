import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/tauri/env";

/** Misma forma que `BatchQueueItem` del store (evita dependencia circular). */
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
};

/** Carga el historial desde `app_data_dir/batch_queue_history.json`. */
export async function backendLoadBatchQueueHistory(): Promise<BatchQueueSnapshot | null> {
	if (!isTauriRuntime()) return null;
	return invoke<BatchQueueSnapshot | null>("load_batch_queue_history");
}

/** Guarda el snapshot actual (debounced desde el store). */
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
		},
	});
}
