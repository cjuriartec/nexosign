import { LOCAL_API_BASE } from "$lib/config/constants";

/** Body JSON típico `{ "error": "..." }` de la API local. */
export type JsonErrorBody = {
	error?: string;
};

/** Error HTTP de `postBatchSign` / `postBatchSignIntent` con `status` y cuerpo parseado. */
export class LocalApiHttpError extends Error {
	readonly status: number;
	readonly body: unknown;

	constructor(operation: string, status: number, body: unknown) {
		const detail = extractJsonErrorMessage(body);
		super(detail ? `${operation}: ${detail}` : `${operation} (${status})`);
		this.name = "LocalApiHttpError";
		this.status = status;
		this.body = body;
	}
}

export function extractJsonErrorMessage(body: unknown): string | undefined {
	if (
		body &&
		typeof body === "object" &&
		"error" in body &&
		typeof (body as JsonErrorBody).error === "string"
	) {
		return (body as JsonErrorBody).error;
	}
	return undefined;
}

export type HealthResponse = {
	status: string;
	service: string;
	version: string;
};

export type PingResponse = { ok: boolean };

export type BatchSignResponse = {
	job_id: string;
	queued: boolean;
};

/** GET /api/v1/batch/jobs/{job_id}/status — estado autoritativo del proceso Rust. */
export type BatchJobPhase =
	| "queued"
	| "running"
	| "completed"
	| "failed"
	| "cancelled";

export type BatchJobStatusResponse = {
	job_id: string;
	phase: BatchJobPhase;
	actual: number;
	total: number;
	/** Segundos Unix del encolado (expiración máx. 5 min en servidor). */
	queued_at_unix?: number | null;
	current_file_name?: string | null;
	error?: string | null;
};

export type BatchSignBody = {
	cert_id_hex: string;
	/** Rutas absolutas a `.pdf` en el sistema de archivos local (la API valida existencia y tamaño). */
	inputs: string[];
	job_id?: string;
	/** Solo en loopback: desbloquea el token antes de encolar. */
	pin?: string;
	/** Directorio absoluto donde escribir `{stem}_firmado.pdf` (p. ej. carpeta `…_firmados`). */
	output_dir?: string;
	/** Primera página: rejilla 3 columnas (ancho) × 5 filas (alto): `col` 0–2, `row` 0–4. */
	signature_grid?: { col: number; row: number };
	/** Si la firma sigue a `POST /api/v1/batch/sign/intent`, elimina la intención pendiente al encolar. */
	intent_request_id?: string;
	/** PNG del sello en base64 (sin prefijo data URL), mismo diseño que Certificados. */
	signature_seal_png_base64?: string;
};

export type BatchSignIntentBody = {
	inputs: string[];
	output_dir?: string;
};

export type BatchSignIntentResponse = {
	request_id: string;
	deep_link: string;
};

/** GET /health — sin credenciales; CORS debe incluir el origen del frontend en dev. */
export async function fetchHealth(
	baseUrl: string = LOCAL_API_BASE,
): Promise<HealthResponse> {
	const res = await fetch(`${baseUrl}/health`);
	if (!res.ok) {
		throw new Error(`health failed: ${res.status}`);
	}
	return res.json() as Promise<HealthResponse>;
}

/** POST /api/v1/ping */
export async function fetchPing(
	baseUrl: string = LOCAL_API_BASE,
): Promise<PingResponse> {
	const res = await fetch(`${baseUrl}/api/v1/ping`, {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: "{}",
	});
	if (!res.ok) {
		throw new Error(`ping failed: ${res.status}`);
	}
	return res.json() as Promise<PingResponse>;
}

/** POST /api/v1/batch/sign — encola firma PAdES; respuesta inmediata con `job_id`. */
export async function postBatchSign(
	body: BatchSignBody,
	baseUrl: string = LOCAL_API_BASE,
): Promise<BatchSignResponse> {
	const headers: Record<string, string> = {
		"Content-Type": "application/json",
	};
	// En tests Node (sin navegador) el header `Origin` no se añade solo; la API exige `Origin` en batch.
	if (typeof window === "undefined") {
		headers["Origin"] = "http://localhost:1420";
	}
	const res = await fetch(`${baseUrl}/api/v1/batch/sign`, {
		method: "POST",
		headers,
		body: JSON.stringify(body),
	});
	if (!res.ok) {
		const err = await res.json().catch(() => ({}));
		throw new LocalApiHttpError("Firma por lotes", res.status, err);
	}
	return res.json() as Promise<BatchSignResponse>;
}

/** GET /api/v1/batch/jobs/{job_id}/status */
export async function fetchBatchJobStatus(
	jobId: string,
	baseUrl: string = LOCAL_API_BASE,
): Promise<BatchJobStatusResponse> {
	const headers: Record<string, string> = {};
	if (typeof window === "undefined") {
		headers["Origin"] = "http://localhost:1420";
	}
	const res = await fetch(
		`${baseUrl}/api/v1/batch/jobs/${encodeURIComponent(jobId)}/status`,
		{ headers },
	);
	if (!res.ok) {
		const err = await res.json().catch(() => ({}));
		throw new LocalApiHttpError("Estado del trabajo batch", res.status, err);
	}
	return res.json() as Promise<BatchJobStatusResponse>;
}

export async function postBatchSignIntent(
	body: BatchSignIntentBody,
	baseUrl: string = LOCAL_API_BASE,
): Promise<BatchSignIntentResponse> {
	const headers: Record<string, string> = {
		"Content-Type": "application/json",
	};
	if (typeof window === "undefined") {
		headers["Origin"] = "http://localhost:1420";
	}
	const res = await fetch(`${baseUrl}/api/v1/batch/sign/intent`, {
		method: "POST",
		headers,
		body: JSON.stringify(body),
	});
	if (!res.ok) {
		const err = await res.json().catch(() => ({}));
		throw new LocalApiHttpError("Registro de intención de firma", res.status, err);
	}
	return res.json() as Promise<BatchSignIntentResponse>;
}

/**
 * POST /api/v1/batch/sign/intent con multipart (`file` / `files` + `output_dir` opcional).
 * No establecer `Content-Type` manualmente: el navegador añade boundary en FormData.
 */
export async function postBatchSignIntentFormData(
	formData: FormData,
	baseUrl: string = LOCAL_API_BASE,
): Promise<BatchSignIntentResponse> {
	const headers: Record<string, string> = {};
	if (typeof window === "undefined") {
		headers["Origin"] = "http://localhost:1420";
	}
	const res = await fetch(`${baseUrl}/api/v1/batch/sign/intent`, {
		method: "POST",
		headers,
		body: formData,
	});
	if (!res.ok) {
		const err = await res.json().catch(() => ({}));
		throw new LocalApiHttpError("Registro de intención de firma (multipart)", res.status, err);
	}
	return res.json() as Promise<BatchSignIntentResponse>;
}

/** POST /api/v1/demo-progress — dispara evento Tauri `progreso` (stub). */
export async function requestDemoProgress(
	baseUrl: string = LOCAL_API_BASE,
	jobId?: string,
): Promise<{ emitted?: boolean; error?: string }> {
	const res = await fetch(`${baseUrl}/api/v1/demo-progress`, {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({ job_id: jobId ?? null }),
	});
	return res.json() as Promise<{ emitted?: boolean; error?: string }>;
}
