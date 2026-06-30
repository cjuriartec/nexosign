/**
 * Notificaciones del sistema (Windows/macOS/Linux) en Tauri; Notification API o consola en navegador.
 * Sustituye los toasts inferiores que tapaban botones de la UI.
 */

import { isTauriRuntime } from "$lib/tauri/env";

const APP_TITLE = "NexoSign";

export type NotifyOptions = {
	description?: string;
};

let permissionChecked = false;
let permissionGranted = false;

function truncate(text: string, max: number): string {
	return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

/** Solicita permiso de notificaciones (idempotente). Llamar al arrancar la app. */
export async function ensureNotificationPermission(): Promise<boolean> {
	if (permissionChecked) return permissionGranted;

	if (isTauriRuntime()) {
		try {
			const { isPermissionGranted, requestPermission } = await import(
				"@tauri-apps/plugin-notification"
			);
			permissionGranted = await isPermissionGranted();
			if (!permissionGranted) {
				const result = await requestPermission();
				permissionGranted = result === "granted";
			}
		} catch {
			permissionGranted = false;
		}
	} else if (typeof Notification !== "undefined") {
		if (Notification.permission === "default") {
			const result = await Notification.requestPermission();
			permissionGranted = result === "granted";
		} else {
			permissionGranted = Notification.permission === "granted";
		}
	}

	permissionChecked = true;
	return permissionGranted;
}

async function sendNative(title: string, body?: string): Promise<void> {
	if (isTauriRuntime()) {
		if (!(await ensureNotificationPermission())) return;
		const { sendNotification } = await import("@tauri-apps/plugin-notification");
		await sendNotification({ title, body });
		return;
	}

	if (typeof Notification !== "undefined" && Notification.permission === "granted") {
		new Notification(title, { body });
		return;
	}

	const line = body ? `${title} — ${body}` : title;
	console.info(`[${APP_TITLE}]`, line);
}

function fire(title: string, body?: string): void {
	void sendNative(title, body).catch(() => {
		/* no bloquear la UI si falla el centro de notificaciones */
	});
}

/** API compatible con los call sites que usaban svelte-sonner. */
export const toast = {
	success(message: string, opts?: NotifyOptions) {
		fire(truncate(message, 120), opts?.description);
	},
	error(message: string, opts?: NotifyOptions) {
		const short = truncate(String(message), 120);
		const desc =
			opts?.description && opts.description.length <= 200 ? opts.description : undefined;
		fire(`Error: ${short}`, desc);
	},
	message(message: string, opts?: NotifyOptions) {
		fire(truncate(message, 120), opts?.description);
	},
	warning(message: string, opts?: NotifyOptions) {
		fire(truncate(message, 120), opts?.description);
	},
};
