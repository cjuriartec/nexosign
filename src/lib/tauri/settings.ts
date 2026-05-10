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
	/** Tauri serializa argumentos en camelCase (`job_id` en Rust → `jobId` desde JS). */
	return invoke<boolean>("cancel_batch_job", { jobId: job_id });
}

/** Rutas PKCS#11 guardadas en SQLite (orden = prioridad respecto a las incorporadas en driver). */
export async function listPkcs11DriverPaths(): Promise<string[]> {
	return invoke<string[]>("list_pkcs11_driver_paths");
}

export async function addPkcs11DriverPath(path: string): Promise<void> {
	return invoke<void>("add_pkcs11_driver_path", { path });
}

export async function removePkcs11DriverPath(path: string): Promise<void> {
	return invoke<void>("remove_pkcs11_driver_path", { path });
}

export async function setPkcs11DriverPathsOrder(paths: string[]): Promise<void> {
	return invoke<void>("set_pkcs11_driver_paths_order", { paths });
}

export async function resetPkcs11DriverPathsToDefaults(): Promise<void> {
	return invoke<void>("reset_pkcs11_driver_paths_to_defaults");
}

export async function getPkcs11PreferredModule(): Promise<string | null> {
	return invoke<string | null>("get_pkcs11_preferred_module");
}

export async function setPkcs11PreferredModule(path: string | null): Promise<void> {
	return invoke<void>("set_pkcs11_preferred_module", { path });
}

export async function listPkcs11EffectiveModulePaths(): Promise<string[]> {
	return invoke<string[]>("list_pkcs11_effective_module_paths");
}

export type ClearLocalApiTempCacheResult = {
	intentUploadsRemoved: boolean;
	batchSignedRemoved: boolean;
	signedJobPathsCleared: boolean;
};

/** Borra carpetas temporales de la API local y vacía rutas de descarga HTTP en RAM. */
export async function clearLocalApiTempCache(): Promise<ClearLocalApiTempCacheResult> {
	return invoke<ClearLocalApiTempCacheResult>("clear_local_api_temp_cache");
}
