import { toast } from "svelte-sonner";
import { cancelBatchJob } from "$lib/tauri/settings";
import { isTauriRuntime } from "$lib/tauri/env";
import { batchQueue, upsertBatchQueueItem } from "$lib/stores/batch-queue.svelte";

export async function cancelActiveBatchJob(): Promise<void> {
	const id = batchQueue.activeBatchJobId;
	if (!id) {
		toast.message("No hay una firma reciente en cola.");
		return;
	}
	if (!isTauriRuntime()) return;
	try {
		upsertBatchQueueItem(id, { status: "cancelling" });
		const ok = await cancelBatchJob(id);
		upsertBatchQueueItem(id, { status: ok ? "cancelled" : "running" });
		toast.message(ok ? "Cancelación enviada" : "Trabajo no encontrado");
	} catch (e) {
		upsertBatchQueueItem(id, { status: "running" });
		toast.error(String(e));
	}
}
