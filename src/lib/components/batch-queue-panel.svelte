<script lang="ts">
	import * as Card from "$lib/components/ui/card/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import {
		batchQueue,
		intentQueue,
		computeBatchQueueHasActiveWork,
	} from "$lib/stores/batch-queue.svelte";

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
				<div
					class="bg-muted/30 border-border/60 flex items-center justify-between gap-2 rounded border px-2.5 py-2 text-xs"
				>
					<div class="min-w-0">
						<p class="truncate font-mono text-[11px]" title={q.jobId}>{q.jobId}</p>
						<p class="text-muted-foreground truncate">{q.label}</p>
					</div>
					<div class="flex items-center gap-2">
						<Badge variant="secondary" class="h-5 text-[10px]">
							{q.status === "preparing"
								? "Prep."
								: q.status === "queued"
									? "Cola"
									: q.status === "running"
										? "Firma"
										: q.status === "cancelling"
											? "Cancel…"
											: q.status === "cancelled"
												? "Cancel."
												: q.status === "finished"
													? "OK"
													: "Error"}
						</Badge>
						<span class="text-muted-foreground tabular-nums text-[11px]">{q.progressPct}%</span>
					</div>
				</div>
			{/each}
		</Card.Content>
	</Card.Root>
{/if}
