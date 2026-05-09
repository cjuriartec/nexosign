<script lang="ts">
	import { onMount } from "svelte";
	import { goto } from "$app/navigation";
	import { listen } from "@tauri-apps/api/event";
	import { invoke } from "@tauri-apps/api/core";
	import { ask } from "@tauri-apps/plugin-dialog";

	let { children } = $props();

	onMount(() => {
		const unsubs: (() => void)[] = [];

		void (async () => {
			try {
				unsubs.push(
					await listen<{ origin: string }>(
						"origin_trust_request",
						async (event) => {
							const origin = event.payload.origin;
							const ok = await ask(
								`¿Confiar en el origen «${origin}» para encolar firma batch vía la API local?`,
								{
									title: "NexoSign — origen no reconocido",
									kind: "warning",
								},
							);
							if (ok) {
								await invoke("add_allowed_origin", { origin });
							}
						},
					),
				);
				unsubs.push(
					await listen("nexosign-deep-link", async () => {
						await goto("/batch");
					}),
				);
			} catch {
				/* Entorno web sin Tauri o sin plugins */
			}
		})();

		return () => {
			for (const u of unsubs) u();
		};
	});
</script>

{@render children()}
