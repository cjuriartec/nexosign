import { afterEach, describe, expect, it, vi } from "vitest";

const onDragDropEvent = vi.fn();

vi.mock("$lib/tauri/env", () => ({
	isTauriRuntime: vi.fn(),
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
	getCurrentWebviewWindow: vi.fn(() => ({
		onDragDropEvent,
	})),
}));

import { isTauriRuntime } from "$lib/tauri/env";
import { listenWindowFileDrop } from "./drag-drop";

describe("listenWindowFileDrop", () => {
	afterEach(() => {
		vi.clearAllMocks();
	});

	it("devuelve noop fuera de Tauri", async () => {
		vi.mocked(isTauriRuntime).mockReturnValue(false);
		const unlisten = await listenWindowFileDrop(() => {});
		expect(unlisten).toBeTypeOf("function");
		expect(onDragDropEvent).not.toHaveBeenCalled();
		unlisten();
	});

	it("propaga over, leave y drop", async () => {
		vi.mocked(isTauriRuntime).mockReturnValue(true);
		let handler: ((event: { payload: unknown }) => void) | undefined;
		onDragDropEvent.mockImplementation((cb: typeof handler) => {
			handler = cb;
			return () => {};
		});

		const onDrop = vi.fn();
		const onHover = vi.fn();
		await listenWindowFileDrop(onDrop, onHover);

		handler!({ payload: { type: "over" } });
		expect(onHover).toHaveBeenCalledWith(true);

		handler!({ payload: { type: "leave" } });
		expect(onHover).toHaveBeenCalledWith(false);

		handler!({ payload: { type: "drop", paths: ["/a.pdf"] } });
		expect(onHover).toHaveBeenCalledWith(false);
		expect(onDrop).toHaveBeenCalledWith(["/a.pdf"]);
	});

	it("ignora drop sin rutas", async () => {
		vi.mocked(isTauriRuntime).mockReturnValue(true);
		let handler: ((event: { payload: unknown }) => void) | undefined;
		onDragDropEvent.mockImplementation((cb: typeof handler) => {
			handler = cb;
			return () => {};
		});

		const onDrop = vi.fn();
		await listenWindowFileDrop(onDrop);
		handler!({ payload: { type: "drop", paths: [] } });
		expect(onDrop).not.toHaveBeenCalled();
	});
});
