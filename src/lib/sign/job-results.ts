import type { ProgressPayload } from "$lib/events/progress";
import { pdfBasenameFromPath } from "$lib/sign/path-util";

export type SignJobFileResult = {
	index: number;
	label: string;
	inputPath?: string;
	outputPath?: string;
	error?: string;
};

export type SignJobFileDisplayStatus = "idle" | "pending" | "ok" | "error";

export type SignJobFileDisplay = SignJobFileResult & {
	status: SignJobFileDisplayStatus;
};

export function labelFromProgressPayload(p: Pick<ProgressPayload, "actual" | "nombre_archivo" | "path">): string {
	const tail = p.nombre_archivo || p.path || "";
	const base = tail.replace(/^.*[/\\]/, "") || tail;
	return base || `Documento ${p.actual}`;
}

/** Actualiza o inserta el resultado de un documento (índice 1-based, como `actual` en progreso). */
export function upsertJobFileResult(
	results: SignJobFileResult[],
	payload: {
		index: number;
		label: string;
		inputPath?: string;
		outputPath?: string | null;
		error?: string | null;
	},
): SignJobFileResult[] {
	const item: SignJobFileResult = {
		index: payload.index,
		label: payload.label,
		inputPath: payload.inputPath,
		outputPath: payload.outputPath ?? undefined,
		error: payload.error ?? undefined,
	};
	if (payload.error) {
		item.outputPath = undefined;
	}
	if (payload.outputPath) {
		item.error = undefined;
	}

	const pos = results.findIndex((r) => r.index === payload.index);
	if (pos >= 0) {
		const next = [...results];
		next[pos] = { ...next[pos], ...item };
		return next;
	}
	return [...results, item].sort((a, b) => a.index - b.index);
}

/** Combina rutas del lote con resultados de progreso para la lista del paso 5. */
export function buildSignJobFileDisplayList(
	inputPaths: string[],
	results: SignJobFileResult[],
	opts: { signing: boolean },
): SignJobFileDisplay[] {
	return inputPaths.map((inputPath, i) => {
		const index = i + 1;
		const r = results.find((x) => x.index === index);
		const label = r?.label ?? pdfBasenameFromPath(inputPath);
		let status: SignJobFileDisplayStatus = "idle";
		if (r?.error) status = "error";
		else if (r?.outputPath) status = "ok";
		else if (opts.signing) status = "pending";
		return {
			index,
			label,
			inputPath: r?.inputPath ?? inputPath,
			outputPath: r?.outputPath,
			error: r?.error,
			status,
		};
	});
}

/** Carpeta de salida principal tras el lote (modo carpeta o último PDF firmado). */
export function resolveBatchOutputDirectoryHint(
	outputDirForJob: string | null,
	results: SignJobFileResult[],
): { dir: string | null; lastOutputPath: string | null } {
	if (outputDirForJob?.trim()) {
		return { dir: outputDirForJob.trim(), lastOutputPath: null };
	}
	const last = [...results].reverse().find((r) => r.outputPath?.trim());
	const lastOutputPath = last?.outputPath?.trim() ?? null;
	return { dir: null, lastOutputPath };
}
