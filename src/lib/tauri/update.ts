import { invoke } from "@tauri-apps/api/core";

export type AppVersionInfo = {
	current: string;
};

export async function getAppVersion(): Promise<AppVersionInfo> {
	return invoke<AppVersionInfo>("get_app_version");
}

export async function checkForAppUpdates(): Promise<void> {
	return invoke<void>("check_for_app_updates");
}
