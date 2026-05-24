import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { isTauriRuntime } from "$lib/tauri/env";

function assertTauri(): void {
	if (!isTauriRuntime()) {
		throw new Error("Solo disponible en la app de escritorio");
	}
}

/** Abre el Explorador con el PDF firmado seleccionado (recomendado en Windows). */
export async function showSignedOutputInExplorer(outputPath: string): Promise<void> {
	assertTauri();
	const p = outputPath.trim();
	if (!p) throw new Error("Ruta de salida vacía");
	await revealItemInDir(p);
}

/** Abre la carpeta de salida del lote (p. ej. `…_firmados`). */
export async function showOutputDirectoryInExplorer(dirPath: string): Promise<void> {
	assertTauri();
	const dir = dirPath.trim();
	if (!dir) throw new Error("Carpeta de salida vacía");
	await openPath(dir);
}
