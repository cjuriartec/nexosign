import { enumeratePdfsUnderFolder } from "$lib/tauri/batch";

export type IngestedBatchPaths = {
	pdfs: string[];
	sourceMode: "files" | "folder" | null;
	folderPath: string | null;
	outputDirForJob: string | null;
};

async function pathIsDirectory(absPath: string): Promise<boolean> {
	try {
		await enumeratePdfsUnderFolder(absPath);
		return true;
	} catch (e) {
		const msg = String(e).toLowerCase();
		if (msg.includes("no es un directorio")) return false;
		throw e;
	}
}

function isPdfPath(path: string): boolean {
	return /\.pdf$/i.test(path.trim());
}

/** Resuelve rutas soltadas (archivos, carpetas o mezcla) en PDFs listos para firmar. */
export async function ingestDroppedPaths(
	rawPaths: string[],
	computeFirmadosDir: (folderAbs: string) => Promise<string>,
): Promise<IngestedBatchPaths> {
	const unique = [...new Set(rawPaths.map((p) => p.trim()).filter(Boolean))];
	if (unique.length === 0) {
		return { pdfs: [], sourceMode: null, folderPath: null, outputDirForJob: null };
	}

	const dirs: string[] = [];
	const files: string[] = [];

	for (const path of unique) {
		if (await pathIsDirectory(path)) {
			dirs.push(path);
		} else if (isPdfPath(path)) {
			files.push(path);
		}
	}

	const pdfsFromDirs: string[] = [];
	for (const dir of dirs) {
		const listed = await enumeratePdfsUnderFolder(dir);
		pdfsFromDirs.push(...listed);
	}

	const allPdfs = [...new Set([...files, ...pdfsFromDirs])];

	if (dirs.length === 1 && files.length === 0) {
		const folderPath = dirs[0];
		return {
			pdfs: allPdfs,
			sourceMode: "folder",
			folderPath,
			outputDirForJob: await computeFirmadosDir(folderPath),
		};
	}

	return {
		pdfs: allPdfs,
		sourceMode: allPdfs.length > 0 ? "files" : null,
		folderPath: null,
		outputDirForJob: null,
	};
}
