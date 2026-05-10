import type { SigningCertSummary } from "$lib/tauri/pkcs11";

/** Imagen oficial por defecto, servida desde `/static`. */
export const DEFAULT_SIGNATURE_IMAGE = "/signature-default.png";

export const SIGNATURE_APPEARANCE_STORAGE_KEY = "nexosign_signature_appearance_v1";

/** Payload JSON compartido (legacy); preferir *_PALETTE / *_EDITOR para DnD en WebKit. */
export const SIG_DRAG_MIME = "application/x-nexo-sig";

/** MIME distintos para que `dataTransfer.types` en dragover fije copy vs move (WKWebView). */
export const SIG_DRAG_PAYLOAD_PALETTE = "application/x-nexo-sig-palette";
export const SIG_DRAG_PAYLOAD_EDITOR = "application/x-nexo-sig-editor";

export type SigTokenId = "firmante" | "fecha_corta" | "fecha_larga" | "certificado";

/** Fragmento del sello: datos dinámicos, texto libre o salto de línea en la vista previa. */
export type SigPart =
	| { kind: "token"; id: SigTokenId }
	| { kind: "text"; value: string }
	| { kind: "break" };

export type SignatureAppearanceState = {
	version: 2;
	imageMode: "bundled" | "custom";
	customImageDataUrl: string | null;
	parts: SigPart[];
};

export const SIG_TOKEN_OPTIONS: {
	id: SigTokenId;
	label: string;
}[] = [
	{ id: "firmante", label: "Firmante" },
	{ id: "fecha_corta", label: "Fecha" },
	{ id: "fecha_larga", label: "Fecha larga" },
	{ id: "certificado", label: "Certificado" },
];

/** Chips que puedes arrastrar desde la paleta (clon al soltar). */
export const PALETTE_ITEMS: { label: string; part: SigPart }[] = [
	...SIG_TOKEN_OPTIONS.map((o) => ({ label: o.label, part: { kind: "token" as const, id: o.id } })),
	{ label: "Salto", part: { kind: "break" } },
	{ label: "Texto", part: { kind: "text", value: "" } },
];

export function defaultSignatureAppearance(): SignatureAppearanceState {
	return {
		version: 2,
		imageMode: "bundled",
		customImageDataUrl: null,
		parts: [
			{ kind: "token", id: "firmante" },
			{ kind: "break" },
			{ kind: "token", id: "fecha_larga" },
			{ kind: "break" },
			{ kind: "token", id: "certificado" },
		],
	};
}

export function loadSignatureAppearance(): SignatureAppearanceState {
	if (typeof localStorage === "undefined") return defaultSignatureAppearance();
	try {
		const raw = localStorage.getItem(SIGNATURE_APPEARANCE_STORAGE_KEY);
		if (!raw) return defaultSignatureAppearance();
		const data = JSON.parse(raw) as Record<string, unknown>;
		return migrateStored(data);
	} catch {
		return defaultSignatureAppearance();
	}
}

export function saveSignatureAppearance(state: SignatureAppearanceState): void {
	if (typeof localStorage === "undefined") return;
	try {
		localStorage.setItem(SIGNATURE_APPEARANCE_STORAGE_KEY, JSON.stringify(state));
	} catch {
		/* quota */
	}
}

function migrateStored(data: Record<string, unknown>): SignatureAppearanceState {
	const base = defaultSignatureAppearance();
	const imageMode = data.imageMode === "custom" ? "custom" : "bundled";
	const customImageDataUrl =
		typeof data.customImageDataUrl === "string" ? data.customImageDataUrl : null;

	if (data.version === 2 && Array.isArray(data.parts)) {
		const parts = sanitizeParts(data.parts);
		return {
			version: 2,
			imageMode,
			customImageDataUrl,
			parts: parts.length ? parts : base.parts,
		};
	}

	if (data.version === 1 && Array.isArray(data.lines)) {
		const lines = sanitizeLegacyLines(data.lines);
		const parts = linesToParts(lines);
		return {
			version: 2,
			imageMode,
			customImageDataUrl,
			parts: parts.length ? parts : base.parts,
		};
	}

	return defaultSignatureAppearance();
}

/** Antigua estructura por líneas (v1). */
type LegacyLine = SigPart[];

function linesToParts(lines: LegacyLine[]): SigPart[] {
	const out: SigPart[] = [];
	for (let i = 0; i < lines.length; i++) {
		if (i > 0) out.push({ kind: "break" });
		out.push(...lines[i]);
	}
	return out;
}

function sanitizeLegacyLines(raw: unknown): LegacyLine[] {
	if (!Array.isArray(raw)) return [];
	const out: LegacyLine[] = [];
	for (const line of raw) {
		if (!Array.isArray(line) || line.length === 0) continue;
		const parts: SigPart[] = [];
		for (const p of line) {
			const sp = sanitizePartUnknown(p);
			if (sp) parts.push(sp);
		}
		if (parts.length) out.push(parts);
	}
	return out.length ? out : [];
}

function sanitizeParts(raw: unknown): SigPart[] {
	if (!Array.isArray(raw)) return [];
	const out: SigPart[] = [];
	for (const p of raw) {
		const sp = sanitizePartUnknown(p);
		if (sp) out.push(sp);
	}
	return out;
}

function sanitizePartUnknown(p: unknown): SigPart | null {
	if (!p || typeof p !== "object") return null;
	const o = p as Record<string, unknown>;
	if (o.kind === "break") return { kind: "break" };
	if (o.kind === "token" && typeof o.id === "string" && isTokenId(o.id)) {
		return { kind: "token", id: o.id };
	}
	if (o.kind === "text" && "value" in o) {
		return { kind: "text", value: String(o.value) };
	}
	return null;
}

function isTokenId(s: unknown): s is SigTokenId {
	return (
		s === "firmante" ||
		s === "fecha_corta" ||
		s === "fecha_larga" ||
		s === "certificado"
	);
}

export function clonePart(part: SigPart): SigPart {
	const j = JSON.parse(JSON.stringify(part)) as SigPart;
	return j;
}

export function extractCnFromDn(dn: string): string | null {
	const m = /(?:^|,)\s*CN=([^,]+)/i.exec(dn.trim());
	return m?.[1]?.trim().replace(/^"|"$/g, "") ?? null;
}

export function getHumanNameFromDn(dn: string): string | null {
	const gnMatch = /(?:^|,)\s*givenName=([^,]+)/i.exec(dn);
	const snMatch = /(?:^|,)\s*surname=([^,]+)/i.exec(dn);
	if (gnMatch && snMatch) {
		return `${gnMatch[1].trim()} ${snMatch[1].trim()}`.replace(/^"|"$/g, "");
	}
	return extractCnFromDn(dn);
}

export function extractDniFromDn(dn: string): string | null {
	const m = /(?:^|,)\s*serialNumber=(?:PNOPE-)?([^,]+)/i.exec(dn);
	return m?.[1]?.trim().replace(/^"|"$/g, "") ?? null;
}

export function extractPurposeFromDn(dn: string): string {
	const cn = extractCnFromDn(dn) || "";
	if (cn.includes(" FIR ")) return "Firma Digital";
	if (cn.includes(" AUT ")) return "Autenticación";
	return "Certificado";
}

function resolveToken(id: SigTokenId, cert: SigningCertSummary | null, now: Date): string {
	switch (id) {
		case "firmante":
			return (
				(cert?.subject_dn ? getHumanNameFromDn(cert.subject_dn) : null) ||
				cert?.label?.trim() ||
				"Firmante"
			);
		case "fecha_corta":
			return now.toLocaleString("es-PE", {
				day: "2-digit",
				month: "2-digit",
				year: "numeric",
				hour: "2-digit",
				minute: "2-digit",
			});
		case "fecha_larga":
			return now.toLocaleString("es-PE", {
				weekday: "long",
				day: "numeric",
				month: "long",
				year: "numeric",
				hour: "2-digit",
				minute: "2-digit",
			});
		case "certificado":
			return (cert?.subject_dn ? extractCnFromDn(cert.subject_dn) : null) || "Certificado";
		default:
			return "";
	}
}

/** Texto para vista previa: una entrada por línea (separadas por chips «Salto»). */
export function resolvePartsToPreviewLines(
	parts: SigPart[],
	cert: SigningCertSummary | null,
	now = new Date(),
): string[] {
	if (parts.length === 0) return ["—"];
	const lines: string[] = [];
	let buf = "";
	const flush = () => {
		const t = buf.trim();
		if (t) lines.push(t);
		buf = "";
	};
	for (const part of parts) {
		if (part.kind === "break") {
			flush();
			continue;
		}
		if (part.kind === "text") {
			buf += part.value;
			continue;
		}
		buf += resolveToken(part.id, cert, now);
	}
	flush();
	return lines.length ? lines : ["—"];
}

export function previewImageSrc(state: SignatureAppearanceState): string {
	if (state.imageMode === "custom" && state.customImageDataUrl) {
		return state.customImageDataUrl;
	}
	return DEFAULT_SIGNATURE_IMAGE;
}

export function tokenLabel(id: SigTokenId): string {
	return SIG_TOKEN_OPTIONS.find((t) => t.id === id)?.label ?? id;
}
