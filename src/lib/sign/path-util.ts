/** Nombre de archivo desde una ruta absoluta (fallback si no hay runtime Tauri `basename`). */
export function pdfBasenameFromPath(path: string): string {
	const parts = path.split(/[/\\]/).filter(Boolean);
	return parts[parts.length - 1] ?? path;
}
