/** Evita que la UI quede en «busy» si un comando Tauri no responde (lector/driver colgado). */
export function invokeWithTimeout<T>(
	promise: Promise<T>,
	ms: number,
	label: string,
): Promise<T> {
	return Promise.race([
		promise,
		new Promise<T>((_, reject) => {
			setTimeout(
				() =>
					reject(
						new Error(
							`${label}: tiempo de espera agotado (${Math.round(ms / 1000)} s). Pulsa «Reconectar lector» o desconecta y vuelve a conectar el DNIe.`,
						),
					),
				ms,
			);
		}),
	]);
}
