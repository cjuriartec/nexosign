<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import type { SignJobFileDisplay } from "$lib/sign/job-results";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { showOutputDirectoryInExplorer, showSignedOutputInExplorer } from "$lib/tauri/open-output";
	import { toast } from "svelte-sonner";
	import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
	import Loader2Icon from "@lucide/svelte/icons/loader-2";
	import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import FileTextIcon from "@lucide/svelte/icons/file-text";

	interface Props {
		items: SignJobFileDisplay[];
		outputDirForJob?: string | null;
		jobSettled?: boolean;
	}

	let {
		items,
		outputDirForJob = null,
		jobSettled = false,
	}: Props = $props();

	const showOutputFolderButton = $derived(
		jobSettled && isTauriRuntime() && !!outputDirForJob?.trim(),
	);

	async function runOpen(fn: () => Promise<void>) {
		try {
			await fn();
		} catch (e) {
			toast.error(String(e));
		}
	}
</script>

{#if items.length > 0}
	<div class="space-y-2">
		<div class="flex flex-wrap items-center justify-between gap-2">
			<p class="text-muted-foreground text-[11px] font-medium uppercase tracking-wide">Documentos</p>
			{#if showOutputFolderButton && outputDirForJob}
				<Button
					type="button"
					variant="outline"
					size="sm"
					class="h-8 gap-1.5 text-xs"
					onclick={() =>
						void runOpen(async () => {
							await showOutputDirectoryInExplorer(outputDirForJob);
						})}
				>
					<FolderOpenIcon class="size-3.5" aria-hidden="true" />
					Abrir carpeta de salida
				</Button>
			{/if}
		</div>

		<ul class="border-border/60 divide-border/40 max-h-48 divide-y overflow-y-auto rounded-lg border">
			{#each items as item (item.index)}
				<li class="flex items-center gap-2 px-2.5 py-2 text-xs">
					<span class="shrink-0" aria-hidden="true">
						{#if item.status === "ok"}
							<CircleCheckIcon class="size-4 text-emerald-600 dark:text-emerald-400" />
						{:else if item.status === "error"}
							<TriangleAlertIcon class="text-destructive size-4" />
						{:else if item.status === "pending"}
							<Loader2Icon class="text-primary size-4 animate-spin" />
						{:else}
							<FileTextIcon class="text-muted-foreground size-4" />
						{/if}
					</span>
					<div class="min-w-0 flex-1">
						<p class="truncate font-medium" title={item.label}>{item.label}</p>
						{#if item.error}
							<p class="text-destructive truncate text-[10px]" title={item.error}>{item.error}</p>
						{/if}
					</div>
					{#if item.outputPath && isTauriRuntime()}
						<Button
							type="button"
							variant="outline"
							size="sm"
							class="h-7 shrink-0 gap-1 px-2 text-[10px]"
							title="Abrir en el Explorador"
							onclick={() =>
								void runOpen(async () => {
									await showSignedOutputInExplorer(item.outputPath!);
								})}
						>
							<FolderOpenIcon class="size-3" aria-hidden="true" />
							Abrir
						</Button>
					{/if}
				</li>
			{/each}
		</ul>
	</div>
{/if}
