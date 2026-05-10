<script lang="ts">
	import Loader2Icon from "@lucide/svelte/icons/loader-2";
	import * as Card from "$lib/components/ui/card/index.js";
	import * as Progress from "$lib/components/ui/progress/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import {
		ACTIVE_BATCH_STATUSES,
		batchQueue,
		intentQueue,
		computeBatchQueueHasActiveWork,
	} from "$lib/stores/batch-queue.svelte";
	import {
		badgeVariantForSidebar,
		batchStatusLabelCompact,
		shortJobIdSidebar,
	} from "$lib/queue/queue-display";
	import { cn } from "$lib/utils.js";

	const batchQueueHasActiveWork = $derived(computeBatchQueueHasActiveWork());

	interface Props {
		showWizardLockHint?: boolean;
	}

	let { showWizardLockHint = false }: Props = $props();

	const showCard = $derived(batchQueue.items.length > 0 || intentQueue.items.length > 0);
</script>

{#if showCard}
	<Card.Root size="sm">
		<Card.Header class="pb-2">
			<Card.Title class="text-sm font-medium">Colas</Card.Title>
			{#if batchQueueHasActiveWork}
				<Card.Description class="text-xs">
					{#if showWizardLockHint}
						Lote activo — atrás bloqueado hasta terminar o cancelar.
					{:else}
						Firma en curso.
					{/if}
				</Card.Description>
			{/if}
		</Card.Header>
		<Card.Content class="space-y-1.5 pt-0">
			{#each intentQueue.items as it}
				<div
					class="bg-muted/30 border-border/60 flex items-center justify-between gap-2 rounded border px-2.5 py-2 text-xs"
				>
					<div class="min-w-0">
						<p class="truncate font-mono text-[11px]" title={it.requestId}>{it.requestId}</p>
						<p class="text-muted-foreground truncate">{it.label}</p>
					</div>
					<Badge variant="outline" class="h-5 text-[10px]">Pendientes</Badge>
				</div>
			{/each}
			{#each batchQueue.items as q}
				{@const isActive = ACTIVE_BATCH_STATUSES.includes(q.status)}
				<div
					class={cn(
						"rounded-lg border px-2.5 py-2 text-xs transition-colors",
						isActive
							? "border-primary/40 bg-linear-to-b from-primary/[0.07] to-transparent"
							: "bg-muted/30 border-border/60",
					)}
				>
					<div class="flex items-start justify-between gap-2">
						<div class="min-w-0 flex-1 space-y-0.5">
							<p class="truncate leading-tight font-medium">{q.label}</p>
							<p class="text-muted-foreground truncate font-mono text-[10px]" title={q.jobId}>
								{shortJobIdSidebar(q.jobId)}
							</p>
						</div>
						<div class="flex shrink-0 flex-col items-end gap-1">
							<div class="flex items-center gap-1">
								{#if q.status === "running"}
									<Loader2Icon
										class="text-primary size-3 shrink-0 animate-spin"
										aria-hidden="true"
									/>
								{/if}
								<Badge variant={badgeVariantForSidebar(q.status)} class="h-5 px-1.5 text-[10px]">
									{batchStatusLabelCompact(q.status)}
								</Badge>
							</div>
							<span
								class={cn(
									"tabular-nums text-[10px]",
									isActive ? "text-primary font-semibold" : "text-muted-foreground",
								)}
							>
								{q.progressPct}%
							</span>
						</div>
					</div>
					{#if isActive}
						<div class="mt-2 space-y-1">
							<Progress.Root class="h-1.5" value={q.progressPct} max={100} />
							<p class="text-muted-foreground text-[10px] leading-tight">
								{#if q.status === "running"}
									Firmando…
								{:else if q.status === "queued"}
									En cola…
								{:else if q.status === "preparing"}
									Preparando…
								{:else}
									Cancelando…
								{/if}
							</p>
						</div>
					{/if}
				</div>
			{/each}
		</Card.Content>
	</Card.Root>
{/if}
