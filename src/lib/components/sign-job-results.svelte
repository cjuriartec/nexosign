<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import type { SignJobFileDisplay } from "$lib/sign/job-results";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { showOutputDirectoryInExplorer, showSignedOutputInExplorer } from "$lib/tauri/open-output";
	import { toast } from "svelte-sonner";
	import { cn } from "$lib/utils.js";
	import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
	import Loader2Icon from "@lucide/svelte/icons/loader-2";
	import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import FileTextIcon from "@lucide/svelte/icons/file-text";
	import ClockIcon from "@lucide/svelte/icons/clock";

	interface Props {
		items: SignJobFileDisplay[];
		outputDirForJob?: string | null;
		jobSettled?: boolean;
		signing?: boolean;
		/** Índice 1-based del documento en curso (evento progreso). */
		activeFileIndex?: number | null;
	}

	let {
		items,
		outputDirForJob = null,
		jobSettled = false,
		signing = false,
		activeFileIndex = null,
	}: Props = $props();

	const okCount = $derived(items.filter((i) => i.status === "ok").length);
	const errorCount = $derived(items.filter((i) => i.status === "error").length);
	const totalCount = $derived(items.length);

	const showOutputFolderButton = $derived(
		jobSettled && isTauriRuntime() && !!outputDirForJob?.trim(),
	);

	const summaryLabel = $derived.by(() => {
		if (!jobSettled) {
			if (signing && activeFileIndex != null) {
				return `Firmando documento ${activeFileIndex} de ${totalCount}`;
			}
			return `${totalCount} documento${totalCount === 1 ? "" : "s"}`;
		}
		if (errorCount === 0) {
			return `${okCount} de ${totalCount} firmado${okCount === 1 ? "" : "s"}`;
		}
		return `${okCount} firmado${okCount === 1 ? "" : "s"}, ${errorCount} con error`;
	});

	function statusBadge(item: SignJobFileDisplay): {
		label: string;
		variant: "default" | "secondary" | "destructive" | "outline";
	} {
		switch (item.status) {
			case "ok":
				return { label: "Firmado", variant: "secondary" };
			case "error":
				return { label: "Error", variant: "destructive" };
			case "pending":
				return { label: "En curso", variant: "default" };
			default:
				return { label: "Pendiente", variant: "outline" };
		}
	}

	async function runOpen(fn: () => Promise<void>) {
		try {
			await fn();
		} catch (e) {
			toast.error(String(e));
		}
	}
</script>

{#if items.length > 0}
	<div class="space-y-3">
		<div class="flex flex-wrap items-center justify-between gap-2">
			<p class="text-muted-foreground text-xs">{summaryLabel}</p>
			{#if showOutputFolderButton && outputDirForJob}
				<Button
					type="button"
					variant="outline"
					size="sm"
					class="h-8 shrink-0 gap-1.5 text-xs"
					onclick={() =>
						void runOpen(async () => {
							await showOutputDirectoryInExplorer(outputDirForJob);
						})}
				>
					<FolderOpenIcon class="size-3.5" aria-hidden="true" />
					Abrir carpeta
				</Button>
			{/if}
		</div>

		<ul class="space-y-2 pb-1" role="list">
			{#each items as item (item.index)}
				{@const badge = statusBadge(item)}
				{@const isActive = signing && activeFileIndex === item.index}
				{@const canOpen =
					item.outputPath && isTauriRuntime() && item.status === "ok"}
				<li
					class={cn(
						"rounded-lg border px-3 py-2.5 transition-colors",
						item.status === "error" && "border-destructive/40 bg-destructive/5",
						item.status === "ok" && "border-emerald-500/30 bg-emerald-500/5",
						isActive && "border-primary/50 bg-primary/5 ring-1 ring-primary/20",
						item.status !== "error" &&
							item.status !== "ok" &&
							!isActive &&
							"border-border/70 bg-muted/15",
					)}
				>
					<div class="flex gap-2.5">
						<span class="mt-0.5 shrink-0" aria-hidden="true">
							{#if item.status === "ok"}
								<CircleCheckIcon class="size-4 text-emerald-600 dark:text-emerald-400" />
							{:else if item.status === "error"}
								<TriangleAlertIcon class="text-destructive size-4" />
							{:else if item.status === "pending" || isActive}
								<Loader2Icon class="text-primary size-4 animate-spin" />
							{:else}
								<ClockIcon class="text-muted-foreground size-4" />
							{/if}
						</span>

						<div class="min-w-0 flex-1 space-y-1.5">
							<p class="truncate text-sm font-medium leading-snug" title={item.label}>
								{item.label}
							</p>

							<div class="flex items-center justify-between gap-2">
								<div class="flex min-w-0 flex-wrap items-center gap-1.5">
									<Badge variant={badge.variant} class="h-5 shrink-0 text-[10px]">
										{badge.label}
									</Badge>
									{#if item.status === "ok" && item.outputPath}
										<span class="text-muted-foreground truncate text-[11px]">
											Guardado en disco
										</span>
									{:else if item.status === "idle" && signing}
										<span class="text-muted-foreground text-[11px]">En cola</span>
									{/if}
								</div>

								{#if canOpen}
									<Button
										type="button"
										variant="outline"
										size="sm"
										class="h-7 shrink-0 gap-1 px-2 text-[11px]"
										title="Abrir en el Explorador"
										onclick={() =>
											void runOpen(async () => {
												await showSignedOutputInExplorer(item.outputPath!);
											})}
									>
										<FolderOpenIcon class="size-3.5" aria-hidden="true" />
										Abrir
									</Button>
								{/if}
							</div>

							{#if item.error}
								<p class="text-destructive text-xs leading-relaxed">{item.error}</p>
							{/if}
						</div>
					</div>
				</li>
			{/each}
		</ul>
	</div>
{:else}
	<div class="text-muted-foreground flex flex-col items-center gap-2 py-8 text-center text-sm">
		<FileTextIcon class="size-8 opacity-40" aria-hidden="true" />
		<p>Sin documentos en este lote.</p>
	</div>
{/if}
