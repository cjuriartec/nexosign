import { invoke } from "@tauri-apps/api/core";

/** Valida rutas como `POST /api/v1/batch/sign` (tamaño, `.pdf`, absolutas). */
export async function validateBatchPdfPaths(paths: string[]): Promise<void> {
	await invoke<void>("validate_batch_pdf_paths", { paths });
}

export type RejectedPdfPath = { path: string; reason: string };

/** Acepta PDFs válidos y devuelve rechazos por archivo (p. ej. tamaño > 50 MiB). */
export async function partitionBatchPdfPaths(
	paths: string[],
): Promise<{ accepted: string[]; rejected: RejectedPdfPath[] }> {
	const [accepted, rejected] = await invoke<[string[], RejectedPdfPath[]]>("partition_batch_pdf_paths", {
		paths,
	});
	return { accepted, rejected };
}
