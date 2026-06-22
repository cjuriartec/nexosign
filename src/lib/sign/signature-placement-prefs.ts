import { SIG_GRID_COLS, SIG_GRID_ROWS } from "$lib/sign/constants";

export const SIGNATURE_PLACEMENT_STORAGE_KEY = "nexosign_signature_placement_v1";
export const LAST_SIGNING_CERT_STORAGE_KEY = "nexosign_last_signing_cert_v1";

export type SignaturePlacement = {
	col: number;
	row: number;
};

const DEFAULT_PLACEMENT: SignaturePlacement = { col: 1, row: 4 };

export function defaultSignaturePlacement(): SignaturePlacement {
	return { ...DEFAULT_PLACEMENT };
}

function clampPlacement(col: number, row: number): SignaturePlacement {
	return {
		col: Math.min(Math.max(0, col), SIG_GRID_COLS - 1),
		row: Math.min(Math.max(0, row), SIG_GRID_ROWS - 1),
	};
}

export function loadSignaturePlacement(): SignaturePlacement {
	if (typeof localStorage === "undefined") return defaultSignaturePlacement();
	try {
		const raw = localStorage.getItem(SIGNATURE_PLACEMENT_STORAGE_KEY);
		if (!raw) return defaultSignaturePlacement();
		const data = JSON.parse(raw) as { col?: unknown; row?: unknown };
		if (typeof data.col !== "number" || typeof data.row !== "number") {
			return defaultSignaturePlacement();
		}
		return clampPlacement(data.col, data.row);
	} catch {
		return defaultSignaturePlacement();
	}
}

export function saveSignaturePlacement(placement: SignaturePlacement): void {
	if (typeof localStorage === "undefined") return;
	try {
		localStorage.setItem(
			SIGNATURE_PLACEMENT_STORAGE_KEY,
			JSON.stringify(clampPlacement(placement.col, placement.row)),
		);
	} catch {
		/* quota */
	}
}

/** Número de casilla en rejilla 3×5 (1–15). */
export function signaturePlacementCellNumber(col: number, row: number): number {
	return row * SIG_GRID_COLS + col + 1;
}

export function loadLastSigningCertId(): string | null {
	if (typeof localStorage === "undefined") return null;
	try {
		const id = localStorage.getItem(LAST_SIGNING_CERT_STORAGE_KEY);
		return id?.trim() || null;
	} catch {
		return null;
	}
}

export function saveLastSigningCertId(certIdHex: string): void {
	if (typeof localStorage === "undefined") return;
	const trimmed = certIdHex.trim();
	if (!trimmed) return;
	try {
		localStorage.setItem(LAST_SIGNING_CERT_STORAGE_KEY, trimmed);
	} catch {
		/* quota */
	}
}
