/**
 * Llamadas al núcleo batch / health del proceso NexoSign vía `invoke` (misma lógica que la API HTTP en :14500).
 * La UI de la app debe usar esto en lugar de `fetch` al loopback para evitar CORS y contenido mixto en release.
 */
import { invoke } from "@tauri-apps/api/core";
import type {
	BatchJobPhase,
	BatchJobStatusResponse,
	BatchSignBody,
	BatchSignResponse,
	HealthResponse,
	PingResponse,
} from "$lib/api/local-api";

/** Error devuelto por comandos `local_api_*` (serde camelCase). */
export class LocalBackendInvokeError extends Error {
	readonly code: string;
	readonly detail: string;

	constructor(code: string, detail: string) {
		super(detail || code);
		this.name = "LocalBackendInvokeError";
		this.code = code;
		this.detail = detail;
	}
}

function throwInvokeErr(e: unknown): never {
	if (e && typeof e === "object" && "code" in e && "detail" in e) {
		const o = e as { code: unknown; detail: unknown };
		throw new LocalBackendInvokeError(String(o.code), String(o.detail ?? ""));
	}
	throw e;
}

/** Mapea fase del snapshot IPC al tipo usado por la cola (mismos literales que el JSON HTTP). */
function normalizePhase(p: string): BatchJobPhase {
	if (
		p === "queued" ||
		p === "running" ||
		p === "completed" ||
		p === "failed" ||
		p === "cancelled"
	) {
		return p;
	}
	if (p === "Queued") return "queued";
	if (p === "Running") return "running";
	if (p === "Completed") return "completed";
	if (p === "Failed") return "failed";
	if (p === "Cancelled") return "cancelled";
	return "running";
}

export async function ipcFetchHealth(): Promise<HealthResponse> {
	try {
		return await invoke<HealthResponse>("local_api_health");
	} catch (e) {
		throwInvokeErr(e);
	}
}

export async function ipcFetchPing(): Promise<PingResponse> {
	try {
		return await invoke<PingResponse>("local_api_ping");
	} catch (e) {
		throwInvokeErr(e);
	}
}

type BatchSignIpcRaw = { jobId?: string; job_id?: string; queued: boolean };

export async function ipcPostBatchSign(body: BatchSignBody): Promise<BatchSignResponse> {
	try {
		const r = await invoke<BatchSignIpcRaw>("local_api_enqueue_batch_sign", { body });
		const jobId = r.jobId ?? r.job_id;
		if (!jobId) throw new Error("respuesta sin job_id");
		return { job_id: jobId, queued: r.queued };
	} catch (e) {
		throwInvokeErr(e);
	}
}

type BatchJobStatusIpcRaw = {
	jobId?: string;
	job_id?: string;
	phase: string;
	actual: number;
	total: number;
	queuedAtUnix?: number | null;
	queued_at_unix?: number | null;
	currentFileName?: string | null;
	current_file_name?: string | null;
	error?: string | null;
	terminalAtUnix?: number | null;
	terminal_at_unix?: number | null;
};

export async function ipcFetchBatchJobStatus(jobId: string): Promise<BatchJobStatusResponse> {
	try {
		const r = await invoke<BatchJobStatusIpcRaw>("local_api_batch_job_status", { jobId });
		const id = r.jobId ?? r.job_id ?? jobId;
		return {
			job_id: id,
			phase: normalizePhase(r.phase),
			actual: r.actual,
			total: r.total,
			queued_at_unix: r.queuedAtUnix ?? r.queued_at_unix ?? undefined,
			current_file_name: r.currentFileName ?? r.current_file_name ?? undefined,
			error: r.error ?? undefined,
		};
	} catch (e) {
		throwInvokeErr(e);
	}
}
