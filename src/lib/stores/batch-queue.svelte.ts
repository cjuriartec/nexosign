import {
	fetchBatchJobStatus,
	type BatchJobPhase,
} from "$lib/api/local-api";
import {
	backendLoadBatchQueueHistory,
	backendSaveBatchQueueHistory,
	type BatchQueueSnapshot,
} from "$lib/tauri/batch-queue-history";
import { isTauriRuntime } from "$lib/tauri/env";
import { getLocalApiBaseUrl } from "$lib/tauri/settings";

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
	finishedAt?: number;
};

/** Intents `POST …/batch/sign/intent` pendientes de completar el asistente. */
export type IntentQueueItem = {
	requestId: string;
	label: string;
	fileCount: number;
	createdAt: number;
};

const ACTIVE_STATUSES: BatchQueueStatus[] = ["preparing", "queued", "running", "cancelling"];

export const TERMINAL_BATCH_STATUSES: BatchQueueStatus[] = ["finished", "error", "cancelled"];

export const MAX_BATCH_QUEUE_ITEMS = 400;
export const MAX_INTENT_QUEUE_ITEMS = 60;

export const batchQueue = $state({
	items: [] as BatchQueueItem[],
	activeBatchJobId: null as string | null,
});

export const intentQueue = $state({
	items: [] as IntentQueueItem[],
	/** Intent cuyo asistente está en pantalla (si aplica). */
	activeRequestId: null as string | null,
});

let persistTimer: ReturnType<typeof setTimeout> | null = null;

function fullSnapshot(): BatchQueueSnapshot {
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
		intentItems: intentQueue.items.map((it) => ({
			requestId: it.requestId,
			label: it.label,
			fileCount: it.fileCount,
			createdAt: it.createdAt,
		})),
		activeIntentRequestId: intentQueue.activeRequestId,
	};
}

export function schedulePersistBatchQueue(): void {
	if (!isTauriRuntime()) return;
	if (persistTimer !== null) clearTimeout(persistTimer);
	persistTimer = setTimeout(() => {
		persistTimer = null;
		void backendSaveBatchQueueHistory(fullSnapshot());
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

export async function initBatchQueuePersistence(): Promise<void> {
	if (!isTauriRuntime()) return;
	try {
		const data = await backendLoadBatchQueueHistory();
		if (!data) return;
		if (data.items?.length) {
			const normalized = normalizeItemsAfterLoad(data.items as BatchQueueItem[]);
			batchQueue.items = normalized.slice(0, MAX_BATCH_QUEUE_ITEMS);
		}
		batchQueue.activeBatchJobId = null;
		const intents = data.intentItems ?? [];
		if (intents.length > 0) {
			intentQueue.items = intents.slice(0, MAX_INTENT_QUEUE_ITEMS) as IntentQueueItem[];
		}
		intentQueue.activeRequestId = null;
		schedulePersistBatchQueue();
	} catch {
		/* sin archivo o JSON antiguo */
	}
}

export function setActiveBatchJobId(id: string | null): void {
	batchQueue.activeBatchJobId = id;
	schedulePersistBatchQueue();
}

export function computeBatchQueueHasActiveWork(): boolean {
	return batchQueue.items.some((q) => ACTIVE_STATUSES.includes(q.status));
}

function mapBackendPhaseToQueueStatus(phase: BatchJobPhase): BatchQueueStatus {
	switch (phase) {
		case "queued":
			return "queued";
		case "running":
			return "running";
		case "completed":
			return "finished";
		case "failed":
			return "error";
		case "cancelled":
			return "cancelled";
		default:
			return "running";
	}
}

/** Refresca ítems activos contra `GET …/batch/jobs/{job_id}/status` (estado en el proceso NexoSign). */
export async function syncBatchQueueFromApi(baseUrl: string): Promise<void> {
	for (const item of batchQueue.items) {
		if (!ACTIVE_STATUSES.includes(item.status)) continue;
		if (!item.jobId || item.jobId.startsWith("pending-")) continue;
		let snap: Awaited<ReturnType<typeof fetchBatchJobStatus>>;
		try {
			snap = await fetchBatchJobStatus(item.jobId, baseUrl);
		} catch {
			continue;
		}
		const total = Math.max(1, snap.total);
		const pct = Math.min(100, Math.round((100 * snap.actual) / total));
		upsertBatchQueueItem(item.jobId, {
			status: mapBackendPhaseToQueueStatus(snap.phase),
			progressPct: pct,
		});
	}
}

export async function syncBatchQueueFromLocalApi(): Promise<void> {
	if (!isTauriRuntime()) return;
	try {
		const base = await getLocalApiBaseUrl();
		await syncBatchQueueFromApi(base);
	} catch {
		/* API local no disponible */
	}
}

export function upsertBatchQueueItem(jobId: string, patch: Partial<BatchQueueItem>): void {
	const idx = batchQueue.items.findIndex((q) => q.jobId === jobId);
	if (idx < 0) return;
	const next = [...batchQueue.items];
	next[idx] = touchTerminalTimestamp(next[idx], patch);
	batchQueue.items = next;
	schedulePersistBatchQueue();
}

/** Vacía trabajos de firma e intents guardados. */
export function clearBatchQueue(): void {
	batchQueue.items = [];
	batchQueue.activeBatchJobId = null;
	intentQueue.items = [];
	intentQueue.activeRequestId = null;
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

export function clearActiveBatchJobOnly(): void {
	batchQueue.activeBatchJobId = null;
	schedulePersistBatchQueue();
}

// --- Intents (integración web / deep link) ---

export function upsertIntentQueueItem(entry: {
	requestId: string;
	label: string;
	fileCount: number;
	createdAt?: number;
}): void {
	const idx = intentQueue.items.findIndex((i) => i.requestId === entry.requestId);
	const createdAt = idx >= 0 ? intentQueue.items[idx].createdAt : (entry.createdAt ?? Date.now());
	const row: IntentQueueItem = {
		requestId: entry.requestId,
		label: entry.label,
		fileCount: entry.fileCount,
		createdAt,
	};
	if (idx >= 0) {
		const next = [...intentQueue.items];
		next[idx] = row;
		intentQueue.items = next;
	} else {
		intentQueue.items = [row, ...intentQueue.items].slice(0, MAX_INTENT_QUEUE_ITEMS);
	}
	schedulePersistBatchQueue();
}

export function setIntentActiveRequestId(id: string | null): void {
	intentQueue.activeRequestId = id;
	schedulePersistBatchQueue();
}

/** El asistente ya no muestra ese intent (p. ej. usuario eligió PDF a mano); el intent sigue en cola. */
export function intentDetachWizard(): void {
	intentQueue.activeRequestId = null;
	schedulePersistBatchQueue();
}

/** Tras `POST /batch/sign` correcto con `intent_request_id`. */
export function completeIntentQueueItem(requestId: string): void {
	intentQueue.items = intentQueue.items.filter((i) => i.requestId !== requestId);
	if (intentQueue.activeRequestId === requestId) {
		intentQueue.activeRequestId = null;
	}
	schedulePersistBatchQueue();
}

export function removeIntentQueueItem(requestId: string): void {
	intentQueue.items = intentQueue.items.filter((i) => i.requestId !== requestId);
	if (intentQueue.activeRequestId === requestId) {
		intentQueue.activeRequestId = null;
	}
	schedulePersistBatchQueue();
}

/** Solo intents terminados en API = ninguno aquí; opcional limpiar todos los intents. */
export function clearAllIntentQueueItems(): void {
	intentQueue.items = [];
	intentQueue.activeRequestId = null;
	schedulePersistBatchQueue();
}
