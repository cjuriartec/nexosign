<script lang="ts">
	import * as Card from "$lib/components/ui/card/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import { batchQueue, computeBatchQueueHasActiveWork } from "$lib/stores/batch-queue.svelte";

	const batchQueueHasActiveWork = $derived(computeBatchQueueHasActiveWork());

	interface Props {
		/** En Firmar: explica el bloqueo del asistente cuando hay trabajo activo. */
		showWizardLockHint?: boolean;
	}

	let { showWizardLockHint = false }: Props = $props();
</script>

{#if batchQueue.items.length > 0}
	<Card.Root size="sm">
		<Card.Header class="pb-2">
			<Card.Title class="text-sm font-medium">Panel de colas</Card.Title>
			<Card.Description class="text-xs">
				{#if showWizardLockHint && batchQueueHasActiveWork}
					Hay trabajo en curso. No puedes volver a pasos anteriores; puedes cancelar el lote activo.
				{:else if batchQueueHasActiveWork}
					Hay trabajo en curso. Puedes cancelar el lote activo desde aquí o en Firmar.
				{:else}
					Últimos lotes enviados en esta sesión.
				{/if}
			</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-1.5 pt-0">
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
								? "Preparando"
								: q.status === "queued"
									? "En cola"
									: q.status === "running"
										? "Firmando"
										: q.status === "cancelling"
											? "Cancelando"
											: q.status === "cancelled"
												? "Cancelado"
												: q.status === "finished"
													? "Terminado"
													: "Error"}
						</Badge>
						<span class="text-muted-foreground tabular-nums text-[11px]">{q.progressPct}%</span>
					</div>
				</div>
			{/each}
		</Card.Content>
	</Card.Root>
{/if}
