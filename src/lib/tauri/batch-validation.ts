import { invoke } from "@tauri-apps/api/core";

/** Valida rutas como `POST /api/v1/batch/sign` (tamaño, `.pdf`, absolutas). */
export async function validateBatchPdfPaths(paths: string[]): Promise<void> {
	await invoke<void>("validate_batch_pdf_paths", { paths });
}
