import { invoke } from "@tauri-apps/api/core";

/** PDFs bajo `path` (carpeta absoluta), recursivo, ordenados. */
export async function enumeratePdfsUnderFolder(path: string): Promise<string[]> {
	return invoke<string[]>("enumerate_pdfs_under_folder", { path });
}
