import { invoke } from "@tauri-apps/api/core";

export async function getLocalApiBaseUrl(): Promise<string> {
	return invoke<string>("get_local_api_base_url");
}

export async function listAllowedOrigins(): Promise<string[]> {
	return invoke<string[]>("list_allowed_origins");
}

export async function addAllowedOrigin(origin: string): Promise<void> {
	return invoke<void>("add_allowed_origin", { origin });
}

export async function removeAllowedOrigin(origin: string): Promise<void> {
	return invoke<void>("remove_allowed_origin", { origin });
}

export async function cancelBatchJob(job_id: string): Promise<boolean> {
	return invoke<boolean>("cancel_batch_job", { job_id });
}
