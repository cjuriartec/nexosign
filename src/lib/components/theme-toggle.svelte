<script lang="ts">
	import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { resetMode, setMode } from "mode-watcher";
	import MonitorIcon from "@lucide/svelte/icons/monitor";
	import MoonIcon from "@lucide/svelte/icons/moon";
	import SunIcon from "@lucide/svelte/icons/sun";
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger>
		<Button variant="outline" size="icon" class="relative" aria-label="Tema">
			<SunIcon
				class="size-4 rotate-0 scale-100 transition-all dark:-rotate-90 dark:scale-0"
			/>
			<MoonIcon
				class="absolute size-4 rotate-90 scale-0 transition-all dark:rotate-0 dark:scale-100"
			/>
		</Button>
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end" class="w-40">
		<DropdownMenu.Item onclick={() => setMode("light")}>
			<SunIcon class="mr-2 size-4" />
			Claro
		</DropdownMenu.Item>
		<DropdownMenu.Item onclick={() => setMode("dark")}>
			<MoonIcon class="mr-2 size-4" />
			Oscuro
		</DropdownMenu.Item>
		<DropdownMenu.Item onclick={() => resetMode()}>
			<MonitorIcon class="mr-2 size-4" />
			Sistema
		</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>
