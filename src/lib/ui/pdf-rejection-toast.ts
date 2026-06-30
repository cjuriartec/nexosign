import { toast } from "$lib/ui/notify";
import type { RejectedPdfPath } from "$lib/tauri/batch-validation";
import { pdfBasenameFromPath } from "$lib/sign/path-util";

const NAME_PREVIEW_LIMIT = 2;

/** Motivo corto y legible (sin rutas absolutas ni jerga interna). */
export function humanizePdfRejectionReason(reason: string): string {
	let r = reason.trim();
	const pathPrefix = /^[A-Za-z]:\\|^\\\\|^\//;
	if (pathPrefix.test(r)) {
		const sep = r.search(/:\s(?=[^:]+$)/);
		if (sep > 0) r = r.slice(sep + 2).trim();
	}
	const lower = r.toLowerCase();
	if (lower.includes("cabecera pdf") || lower.includes("%pdf")) {
		return "No es un PDF válido";
	}
	if (lower.includes("demasiado grande") || lower.includes("50 mib")) {
		return "Supera 50 MiB";
	}
	if (lower.includes("solo se admiten .pdf")) {
		return "Extensión no válida";
	}
	if (lower.includes("no es un archivo regular")) {
		return "No es un archivo";
	}
	if (lower.includes("la ruta debe ser absoluta")) {
		return "Ruta no válida";
	}
	if (r.length > 72) return `${r.slice(0, 69)}…`;
	return r;
}

/** Mensaje breve para errores de firma en la lista de resultados. */
export function humanizeUserFacingError(message: string): string {
	const lower = message.toLowerCase();
	if (lower.includes("byterange") || lower.includes("hueco")) {
		return "PDF no compatible con firma incremental";
	}
	if (lower.includes("invalid file trailer") || lower.includes("mediabox")) {
		return "PDF dañado o no estándar";
	}
	if (lower.includes("pades") || lower.includes("cms") || lower.includes("pkcs")) {
		return "No se pudo completar la firma";
	}
	return humanizePdfRejectionReason(message);
}

function groupRejections(rejected: RejectedPdfPath[]) {
	const groups = new Map<string, string[]>();
	for (const item of rejected) {
		const name = pdfBasenameFromPath(item.path);
		const reason = humanizePdfRejectionReason(item.reason);
		const names = groups.get(reason) ?? [];
		names.push(name);
		groups.set(reason, names);
	}
	return groups;
}

/** Aviso breve al omitir PDF del lote (no bloquea la pantalla). */
export function toastPdfRejections(rejected: RejectedPdfPath[]): void {
	if (rejected.length === 0) return;

	if (rejected.length === 1) {
		const name = pdfBasenameFromPath(rejected[0].path);
		const reason = humanizePdfRejectionReason(rejected[0].reason);
		toast.warning(`${name} omitido`, {
			description: reason,
		});
		return;
	}

	const groups = groupRejections(rejected);
	const [topReason, topNames] = [...groups.entries()].sort((a, b) => b[1].length - a[1].length)[0]!;
	const uniqueNames = [...new Set(topNames)];
	const preview = uniqueNames.slice(0, NAME_PREVIEW_LIMIT).join(", ");
	const extraNames = uniqueNames.length - NAME_PREVIEW_LIMIT;
	const extraGroups = groups.size - 1;

	let description = preview;
	if (extraNames > 0) description += ` y ${extraNames} más`;
	description += ` · ${topReason}`;
	if (extraGroups > 0) description += ` (+${extraGroups} motivo${extraGroups === 1 ? "" : "s"})`;

	toast.warning(
		`${rejected.length} archivos omitidos`,
		{ description },
	);
}
