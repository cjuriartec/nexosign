import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/tauri/env", () => ({
	isTauriRuntime: () => false,
}));

const sendNotification = vi.fn();
const isPermissionGranted = vi.fn().mockResolvedValue(true);
const requestPermission = vi.fn();

vi.mock("@tauri-apps/plugin-notification", () => ({
	isPermissionGranted,
	requestPermission,
	sendNotification,
}));

import { toast } from "./notify";
import { toastFail, toastInfo, toastWarn } from "./app-toast";

describe("app-toast", () => {
	afterEach(() => {
		vi.clearAllMocks();
	});

	it("toastInfo delega en notify.message", () => {
		const spy = vi.spyOn(toast, "message");
		toastInfo("Listo");
		expect(spy).toHaveBeenCalledWith("Listo", undefined);
	});

	it("toastInfo admite descripción opcional", () => {
		const spy = vi.spyOn(toast, "message");
		toastInfo("Sin PDF", "Revisa la carpeta");
		expect(spy).toHaveBeenCalledWith("Sin PDF", { description: "Revisa la carpeta" });
	});

	it("toastWarn usa warning", () => {
		const spy = vi.spyOn(toast, "warning");
		toastWarn("PDF demasiado grande", "Máximo 50 MiB");
		expect(spy).toHaveBeenCalledWith("PDF demasiado grande", {
			description: "Máximo 50 MiB",
		});
	});

	it("toastFail trunca mensajes largos", () => {
		const spy = vi.spyOn(toast, "error");
		const long = "x".repeat(130);
		toastFail(long);
		expect(spy).toHaveBeenCalledWith(`${"x".repeat(117)}…`, undefined);
	});

	it("toastFail omite descripción si supera 100 caracteres", () => {
		const spy = vi.spyOn(toast, "error");
		toastFail("Error", "d".repeat(101));
		expect(spy).toHaveBeenCalledWith("Error", undefined);
	});

	it("toastFail incluye descripción corta", () => {
		const spy = vi.spyOn(toast, "error");
		toastFail("Origen no autorizado", "Añádelo en Ajustes");
		expect(spy).toHaveBeenCalledWith("Origen no autorizado", {
			description: "Añádelo en Ajustes",
		});
	});
});
