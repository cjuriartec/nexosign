import {
	backendLoadBatchQueueHistory,
	backendSaveBatchQueueHistory,
	type BatchQueueSnapshot,
} from "$lib/tauri/batch-queue-history";
import { isTauriRuntime } from "$lib/tauri/env";

export type BatchQueueStatus =
	| "preparing"
	| "queued"
	| "running"
	| "cancelling"
	| "cancelled"
	| "finished"
	| "error";

export type BatchQueueItem = {
	jobId: string;
	status: BatchQueueStatus;
	label: string;
	progressPct: number;
	createdAt: number;
	/** Marca temporal cuando el ítem llega a un estado terminal (si aplica). */
	finishedAt?: number;
};

const ACTIVE_STATUSES: BatchQueueStatus[] = ["preparing", "queued", "running", "cancelling"];

/** Estados que ya no cambian (historial cerrado). */
export const TERMINAL_BATCH_STATUSES: BatchQueueStatus[] = ["finished", "error", "cancelled"];

/** Tope de entradas en historial (las más recientes se conservan al frente). */
export const MAX_BATCH_QUEUE_ITEMS = 400;

/** Un solo `$state` exportable: se mutan propiedades, no se reasigna el binding exportado. */
export const batchQueue = $state({
	items: [] as BatchQueueItem[],
	activeBatchJobId: null as string | null,
});

let persistTimer: ReturnType<typeof setTimeout> | null = null;

function snapshot(): BatchQueueSnapshot {
	return {
		items: batchQueue.items.map((it) => ({
			jobId: it.jobId,
			status: it.status,
			label: it.label,
			progressPct: it.progressPct,
			createdAt: it.createdAt,
			finishedAt: it.finishedAt,
		})),
		activeBatchJobId: batchQueue.activeBatchJobId,
	};
}

export function schedulePersistBatchQueue(): void {
	if (!isTauriRuntime()) return;
	if (persistTimer !== null) clearTimeout(persistTimer);
	persistTimer = setTimeout(() => {
		persistTimer = null;
		void backendSaveBatchQueueHistory(snapshot());
	}, 450);
}

function touchTerminalTimestamp(prev: BatchQueueItem, next: Partial<BatchQueueItem>): BatchQueueItem {
	const merged = { ...prev, ...next };
	const wasTerminal = TERMINAL_BATCH_STATUSES.includes(prev.status);
	const isTerminal = TERMINAL_BATCH_STATUSES.includes(merged.status);
	if (isTerminal && !wasTerminal && merged.finishedAt === undefined) {
		merged.finishedAt = Date.now();
	}
	return merged as BatchQueueItem;
}

/** Tras reiniciar la app, los trabajos «activos» ya no tienen seguimiento real. */
function normalizeItemsAfterLoad(items: BatchQueueItem[]): BatchQueueItem[] {
	const now = Date.now();
	return items.map((it) => {
		if (ACTIVE_STATUSES.includes(it.status)) {
			return {
				...it,
				status: "error",
				progressPct: it.progressPct,
				label: it.label.includes("(sesión anterior)") ? it.label : `${it.label} (sesión anterior)`,
				finishedAt: it.finishedAt ?? now,
			};
		}
		return it;
	});
}

/** Carga historial desde disco (llamar una vez al arranque en Tauri). */
export async function initBatchQueuePersistence(): Promise<void> {
	if (!isTauriRuntime()) return;
	try {
		const data = await backendLoadBatchQueueHistory();
		if (!data || data.items.length === 0) return;
		const normalized = normalizeItemsAfterLoad(data.items as BatchQueueItem[]);
		batchQueue.items = normalized.slice(0, MAX_BATCH_QUEUE_ITEMS);
		batchQueue.activeBatchJobId = null;
		schedulePersistBatchQueue();
	} catch {
		/* sin archivo o JSON antiguo: se mantiene vacío */
	}
}

export function setActiveBatchJobId(id: string | null): void {
	batchQueue.activeBatchJobId = id;
	schedulePersistBatchQueue();
}

/** No usar `$derived` exportado desde el módulo (regla de Svelte). Para UI reactiva: `$derived(computeBatchQueueHasActiveWork())`. */
export function computeBatchQueueHasActiveWork(): boolean {
	return batchQueue.items.some((q) => ACTIVE_STATUSES.includes(q.status));
}

export function upsertBatchQueueItem(jobId: string, patch: Partial<BatchQueueItem>): void {
	const idx = batchQueue.items.findIndex((q) => q.jobId === jobId);
	if (idx < 0) return;
	const next = [...batchQueue.items];
	next[idx] = touchTerminalTimestamp(next[idx], patch);
	batchQueue.items = next;
	schedulePersistBatchQueue();
}

/** Vacía todo el historial y el trabajo activo (p. ej. «Limpiar todo» con confirmación). */
export function clearBatchQueue(): void {
	batchQueue.items = [];
	batchQueue.activeBatchJobId = null;
	schedulePersistBatchQueue();
}

export function prependBatchQueueItem(item: BatchQueueItem): void {
	const withTs = { ...item };
	if (TERMINAL_BATCH_STATUSES.includes(withTs.status) && withTs.finishedAt === undefined) {
		withTs.finishedAt = Date.now();
	}
	batchQueue.items = [withTs, ...batchQueue.items].slice(0, MAX_BATCH_QUEUE_ITEMS);
	schedulePersistBatchQueue();
}

export function replaceQueueJobId(oldId: string, newId: string, patch: Partial<BatchQueueItem>): void {
	batchQueue.items = batchQueue.items.map((q) =>
		q.jobId === oldId ? touchTerminalTimestamp({ ...q, jobId: newId }, patch) : q,
	);
	if (batchQueue.activeBatchJobId === oldId) {
		batchQueue.activeBatchJobId = newId;
	}
	schedulePersistBatchQueue();
}

export function removeBatchQueueItem(jobId: string): void {
	batchQueue.items = batchQueue.items.filter((q) => q.jobId !== jobId);
	if (batchQueue.activeBatchJobId === jobId) {
		batchQueue.activeBatchJobId = null;
	}
	schedulePersistBatchQueue();
}

/** Quita entradas terminadas (correcto, error o cancelado). */
export function clearTerminalBatchQueueItems(): void {
	batchQueue.items = batchQueue.items.filter((q) => !TERMINAL_BATCH_STATUSES.includes(q.status));
	schedulePersistBatchQueue();
}

export function removeBatchQueueItems(jobIds: string[]): void {
	if (jobIds.length === 0) return;
	const set = new Set(jobIds);
	batchQueue.items = batchQueue.items.filter((q) => !set.has(q.jobId));
	if (batchQueue.activeBatchJobId && set.has(batchQueue.activeBatchJobId)) {
		batchQueue.activeBatchJobId = null;
	}
	schedulePersistBatchQueue();
}

/** Solo desvincula el asistente actual sin borrar el historial (p. ej. «Nuevo lote»). */
export function clearActiveBatchJobOnly(): void {
	batchQueue.activeBatchJobId = null;
	schedulePersistBatchQueue();
}
