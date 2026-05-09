<script lang="ts">
	import "../app.css";
	import { onMount } from "svelte";
	import { goto } from "$app/navigation";
	import { listen } from "@tauri-apps/api/event";
	import { invoke } from "@tauri-apps/api/core";
	import { ask } from "@tauri-apps/plugin-dialog";
	import { toast } from "svelte-sonner";
	import { ModeWatcher } from "mode-watcher";
	import { Toaster } from "$lib/components/ui/sonner/index.js";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";
	import AppSidebar from "$lib/components/app-sidebar.svelte";
	import ThemeToggle from "$lib/components/theme-toggle.svelte";

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
								`¿Confiar en el origen «${origin}» para usar la API local de firma batch?`,
								{
									title: "NexoSign — origen no reconocido",
									kind: "warning",
								},
							);
							if (ok) {
								await invoke("add_allowed_origin", { origin });
								toast.success(`Origen permitido: ${origin}`);
							}
						},
					),
				);
				unsubs.push(
					await listen("nexosign-deep-link", async () => {
						await goto("/sign");
					}),
				);
			} catch {
				/* Sin entorno Tauri */
			}
		})();

		return () => {
			for (const u of unsubs) u();
		};
	});
</script>

<ModeWatcher />
<Toaster richColors position="top-center" />

<Sidebar.Provider>
	<AppSidebar />
	<Sidebar.Inset>
		<header
			class="flex h-14 shrink-0 items-center gap-2 border-b px-4"
			data-slot="layout"
		>
			<Sidebar.Trigger class="-ml-1" />
			<div class="flex flex-1 items-center justify-end gap-2">
				<ThemeToggle />
			</div>
		</header>
		<main class="flex min-h-[calc(100vh-3.5rem)] flex-1 flex-col">
			<div class="mx-auto w-full max-w-6xl flex-1 p-6">
				{@render children?.()}
			</div>
		</main>
	</Sidebar.Inset>
</Sidebar.Provider>
