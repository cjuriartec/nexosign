<script lang="ts">
	import { userPrefersMode, setMode } from "mode-watcher";
	import SunIcon from "@lucide/svelte/icons/sun";
	import MoonIcon from "@lucide/svelte/icons/moon";
	import MonitorIcon from "@lucide/svelte/icons/monitor";
	import { cn } from "$lib/utils.js";

	function pick(next: "light" | "dark" | "system") {
		setMode(next);
	}

	const pref = $derived(userPrefersMode.current);

	function segmentClass(active: boolean): string {
		return cn(
			"flex items-center justify-center gap-1.5 rounded-lg px-3 py-2 text-xs font-medium transition-all outline-none sm:min-w-[5.5rem]",
			"focus-visible:ring-ring focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-offset-background",
			active
				? "bg-background text-foreground shadow-sm ring-1 ring-border/70"
				: "text-muted-foreground hover:bg-muted/80 hover:text-foreground",
		);
	}
</script>

<div
	class="bg-muted/70 inline-flex max-w-full rounded-xl border border-border/70 p-1 shadow-inner"
	role="group"
	aria-label="Tema de la interfaz"
>
	<button
		type="button"
		class={segmentClass(pref === "light")}
		aria-pressed={pref === "light"}
		aria-label="Tema claro"
		onclick={() => pick("light")}
	>
		<SunIcon class="size-4 shrink-0 opacity-90" aria-hidden="true" />
		<span class="hidden sm:inline">Claro</span>
	</button>
	<button
		type="button"
		class={segmentClass(pref === "dark")}
		aria-pressed={pref === "dark"}
		aria-label="Tema oscuro"
		onclick={() => pick("dark")}
	>
		<MoonIcon class="size-4 shrink-0 opacity-90" aria-hidden="true" />
		<span class="hidden sm:inline">Oscuro</span>
	</button>
	<button
		type="button"
		class={segmentClass(pref === "system")}
		aria-pressed={pref === "system"}
		aria-label="Igual que el sistema"
		onclick={() => pick("system")}
	>
		<MonitorIcon class="size-4 shrink-0 opacity-90" aria-hidden="true" />
		<span class="hidden sm:inline">Sistema</span>
	</button>
</div>
