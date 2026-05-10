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
};

const ACTIVE_STATUSES: BatchQueueStatus[] = ["preparing", "queued", "running", "cancelling"];

/** Un solo `$state` exportable: se mutan propiedades, no se reasigna el binding exportado. */
export const batchQueue = $state({
	items: [] as BatchQueueItem[],
	activeBatchJobId: null as string | null,
});

export function setActiveBatchJobId(id: string | null): void {
	batchQueue.activeBatchJobId = id;
}

/** No usar `$derived` exportado desde el módulo (regla de Svelte). Para UI reactiva: `$derived(computeBatchQueueHasActiveWork())`. */
export function computeBatchQueueHasActiveWork(): boolean {
	return batchQueue.items.some((q) => ACTIVE_STATUSES.includes(q.status));
}

export function upsertBatchQueueItem(jobId: string, patch: Partial<BatchQueueItem>): void {
	const idx = batchQueue.items.findIndex((q) => q.jobId === jobId);
	if (idx < 0) return;
	const next = [...batchQueue.items];
	next[idx] = { ...next[idx], ...patch };
	batchQueue.items = next;
}

export function clearBatchQueue(): void {
	batchQueue.items = [];
	batchQueue.activeBatchJobId = null;
}

export function prependBatchQueueItem(item: BatchQueueItem): void {
	batchQueue.items = [item, ...batchQueue.items].slice(0, 10);
}

export function replaceQueueJobId(oldId: string, newId: string, patch: Partial<BatchQueueItem>): void {
	batchQueue.items = batchQueue.items.map((q) =>
		q.jobId === oldId ? { ...q, jobId: newId, ...patch } : q,
	);
}
