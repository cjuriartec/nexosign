import type { BatchQueueStatus } from "$lib/stores/batch-queue.svelte";

/** Etiqueta corta (panel lateral). */
export function batchStatusLabelCompact(s: BatchQueueStatus): string {
	switch (s) {
		case "preparing":
			return "Prep.";
		case "queued":
			return "Cola";
		case "running":
			return "Curso";
		case "cancelling":
			return "…";
		case "cancelled":
			return "Cancel.";
		case "finished":
			return "OK";
		case "error":
			return "Err";
		default:
			return s;
	}
}

/** Etiqueta larga (página Colas). */
export function batchStatusLabelFull(s: BatchQueueStatus): string {
	switch (s) {
		case "preparing":
			return "Preparando";
		case "queued":
			return "En cola";
		case "running":
			return "En curso";
		case "cancelling":
			return "Cancelando";
		case "cancelled":
			return "Cancelado";
		case "finished":
			return "Completado";
		case "error":
			return "Error";
		default:
			return s;
	}
}

export function badgeVariantForBatchStatus(
	s: BatchQueueStatus,
): "default" | "secondary" | "destructive" | "outline" {
	switch (s) {
		case "running":
		case "queued":
		case "preparing":
			return "default";
		case "cancelling":
			return "secondary";
		case "error":
			return "destructive";
		default:
			return "outline";
	}
}

/** Variante para badges compactos (panel): mismos colores que la página Colas. */
export function badgeVariantForSidebar(
	s: BatchQueueStatus,
): "default" | "secondary" | "destructive" | "outline" {
	return badgeVariantForBatchStatus(s);
}

export function shortJobIdSidebar(id: string): string {
	if (id.length <= 14) return id;
	return `${id.slice(0, 6)}…${id.slice(-4)}`;
}

export function shortJobIdWide(id: string): string {
	if (id.length <= 18) return id;
	return `${id.slice(0, 10)}…${id.slice(-6)}`;
}
