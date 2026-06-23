import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("svelte-sonner", () => ({
	toast: {
		message: vi.fn(),
		warning: vi.fn(),
		error: vi.fn(),
	},
}));

import { toast } from "svelte-sonner";
import { toastFail, toastInfo, toastWarn } from "./app-toast";

const mockToast = toast as {
	message: ReturnType<typeof vi.fn>;
	warning: ReturnType<typeof vi.fn>;
	error: ReturnType<typeof vi.fn>;
};

describe("app-toast", () => {
	afterEach(() => {
		vi.clearAllMocks();
	});

	it("toastInfo usa message con duración por defecto", () => {
		toastInfo("Listo");
		expect(mockToast.message).toHaveBeenCalledWith("Listo", { duration: 4500 });
	});

	it("toastInfo admite descripción opcional", () => {
		toastInfo("Sin PDF", "Revisa la carpeta");
		expect(mockToast.message).toHaveBeenCalledWith("Sin PDF", {
			duration: 4500,
			description: "Revisa la carpeta",
		});
	});

	it("toastWarn usa warning", () => {
		toastWarn("PDF demasiado grande", "Máximo 50 MiB");
		expect(mockToast.warning).toHaveBeenCalledWith("PDF demasiado grande", {
			duration: 4500,
			description: "Máximo 50 MiB",
		});
	});

	it("toastFail trunca mensajes largos", () => {
		const long = "x".repeat(130);
		toastFail(long);
		expect(mockToast.error).toHaveBeenCalledWith(`${"x".repeat(117)}…`, {
			duration: 6000,
		});
	});

	it("toastFail omite descripción si supera 100 caracteres", () => {
		toastFail("Error", "d".repeat(101));
		expect(mockToast.error).toHaveBeenCalledWith("Error", { duration: 6000 });
	});

	it("toastFail incluye descripción corta", () => {
		toastFail("Origen no autorizado", "Añádelo en Ajustes");
		expect(mockToast.error).toHaveBeenCalledWith("Origen no autorizado", {
			duration: 6000,
			description: "Añádelo en Ajustes",
		});
	});
});
