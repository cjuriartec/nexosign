import { LOCAL_API_BASE } from "$lib/config/constants";

export type HealthResponse = {
	status: string;
	service: string;
	version: string;
};

export type PingResponse = { ok: boolean };

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
