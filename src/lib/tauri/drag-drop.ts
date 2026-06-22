import { isTauriRuntime } from "$lib/tauri/env";

export type FileDropState = "idle" | "over" | "drop";

/** Escucha soltar archivos/carpetas en la ventana (solo Tauri). */
export async function listenWindowFileDrop(
	onDrop: (paths: string[]) => void,
	onHover?: (over: boolean) => void,
): Promise<() => void> {
	if (!isTauriRuntime()) return () => {};

	const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
	const webview = getCurrentWebviewWindow();

	return webview.onDragDropEvent((event) => {
		const payload = event.payload;
		if (payload.type === "over") {
			onHover?.(true);
			return;
		}
		if (payload.type === "leave") {
			onHover?.(false);
			return;
		}
		if (payload.type === "drop") {
			onHover?.(false);
			if (payload.paths.length > 0) onDrop(payload.paths);
		}
	});
}
