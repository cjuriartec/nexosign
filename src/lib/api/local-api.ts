import { LOCAL_API_BASE } from "$lib/config/constants";

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

export type BatchSignBody = {
	cert_id_hex: string;
	/** Rutas absolutas a `.pdf` en el sistema de archivos local (la API valida existencia y tamaño). */
	inputs: string[];
	job_id?: string;
	/** Solo en loopback: desbloquea el token antes de encolar. */
	pin?: string;
	/** Directorio absoluto donde escribir `{stem}_firmado.pdf` (p. ej. carpeta `…_firmados`). */
	output_dir?: string;
	/** Primera página: casilla en rejilla 5×7 (`col` 0–4 izquierda→derecha, `row` 0–6 arriba→abajo). */
	signature_grid?: { col: number; row: number };
	/** Si la firma sigue a `POST /api/v1/batch/sign/intent`, elimina la intención pendiente al encolar. */
	intent_request_id?: string;
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
		throw new Error(
			`batch sign failed: ${res.status} ${JSON.stringify(err)}`,
		);
	}
	return res.json() as Promise<BatchSignResponse>;
}

/** POST /api/v1/batch/sign/intent — registra PDFs para firmar tras el asistente en la app (no encola aún). */
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
		throw new Error(`batch sign intent failed: ${res.status} ${JSON.stringify(err)}`);
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
