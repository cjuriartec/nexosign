<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import BatchQueuePanel from "$lib/components/batch-queue-panel.svelte";
	import { cancelActiveBatchJob } from "$lib/batch/cancel-active-batch";
	import { batchQueue } from "$lib/stores/batch-queue.svelte";

	const activeQueueRow = $derived(
		batchQueue.items.find((q) => q.jobId === batchQueue.activeBatchJobId),
	);

	const canCancelActiveJob = $derived(
		batchQueue.activeBatchJobId !== null &&
			activeQueueRow !== undefined &&
			["preparing", "queued", "running", "cancelling"].includes(activeQueueRow.status),
	);
</script>

<svelte:head>
	<title>Colas — NexoSign</title>
</svelte:head>

<div class="space-y-4 pb-6">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<h1 class="text-2xl font-semibold tracking-tight">Colas</h1>
		<div class="flex flex-wrap items-center gap-2">
			{#if canCancelActiveJob}
				<Button type="button" variant="outline" size="sm" onclick={() => void cancelActiveBatchJob()}>
					Cancelar cola
				</Button>
			{/if}
			<Button variant="outline" size="sm" href="/sign">Ir a Firmar</Button>
		</div>
	</div>

	<BatchQueuePanel />

	<p class="text-muted-foreground max-w-xl text-sm">
		Los lotes mostrados corresponden a esta sesión de la aplicación. Para iniciar una nueva firma, abre
		<strong class="text-foreground font-medium">Firmar</strong>.
	</p>
</div>
