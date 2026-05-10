<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import * as ScrollArea from "$lib/components/ui/scroll-area/index.js";
	import { cancelActiveBatchJob } from "$lib/batch/cancel-active-batch";
	import { ask } from "@tauri-apps/plugin-dialog";
	import { toast } from "svelte-sonner";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import { cn } from "$lib/utils.js";
	import {
		batchQueue,
		clearBatchQueue,
		clearTerminalBatchQueueItems,
		removeBatchQueueItem,
		computeBatchQueueHasActiveWork,
		TERMINAL_BATCH_STATUSES,
		type BatchQueueItem,
		type BatchQueueStatus,
	} from "$lib/stores/batch-queue.svelte";

	type QueueFilter = "all" | "active" | "finished" | "cancelled" | "error";

	let filter = $state<QueueFilter>("all");

	const activeQueueRow = $derived(
		batchQueue.items.find((q) => q.jobId === batchQueue.activeBatchJobId),
	);

	const canCancelActiveJob = $derived(
		batchQueue.activeBatchJobId !== null &&
			activeQueueRow !== undefined &&
			["preparing", "queued", "running", "cancelling"].includes(activeQueueRow.status),
	);

	const batchQueueHasActiveWork = $derived(computeBatchQueueHasActiveWork());

	const filteredItems = $derived.by(() => {
		const list = batchQueue.items;
		if (filter === "all") return list;
		if (filter === "active") {
			return list.filter((q) =>
				["preparing", "queued", "running", "cancelling"].includes(q.status),
			);
		}
		if (filter === "finished") return list.filter((q) => q.status === "finished");
		if (filter === "cancelled") return list.filter((q) => q.status === "cancelled");
		return list.filter((q) => q.status === "error");
	});

	const terminalCount = $derived(
		batchQueue.items.filter((q) => TERMINAL_BATCH_STATUSES.includes(q.status)).length,
	);

	function statusLabel(s: BatchQueueStatus): string {
		switch (s) {
			case "preparing":
				return "Preparando";
			case "queued":
				return "En cola";
			case "running":
				return "Firmando";
			case "cancelling":
				return "Cancelando";
			case "cancelled":
				return "Cancelado";
			case "finished":
				return "Completado";
			case "error":
				return "Error";
			default:
				return s;
		}
	}

	function formatWhen(it: BatchQueueItem): string {
		const ts = it.finishedAt ?? it.createdAt;
		try {
			return new Intl.DateTimeFormat("es", {
				dateStyle: "short",
				timeStyle: "short",
			}).format(new Date(ts));
		} catch {
			return "—";
		}
	}

	async function confirmClearAll(): Promise<void> {
		const ok = await ask(
			"Se borrarán todas las entradas del historial de colas en esta aplicación. ¿Continuar?",
			{ title: "Limpiar historial", kind: "warning" },
		);
		if (!ok) return;
		clearBatchQueue();
		toast.success("Historial vaciado.");
	}

	async function confirmClearFinished(): Promise<void> {
		const n = terminalCount;
		if (n === 0) {
			toast.message("No hay lotes finalizados para quitar.");
			return;
		}
		const ok = await ask(
			`Se eliminarán ${n} entrada(s) ya terminadas (completadas, canceladas o con error). ¿Continuar?`,
			{ title: "Quitar finalizados", kind: "info" },
		);
		if (!ok) return;
		clearTerminalBatchQueueItems();
		toast.success("Entradas finalizadas eliminadas.");
	}

	function removeOne(jobId: string): void {
		removeBatchQueueItem(jobId);
		toast.message("Entrada eliminada.");
	}

	const filterButtons: { id: QueueFilter; label: string }[] = [
		{ id: "all", label: "Todos" },
		{ id: "active", label: "En curso" },
		{ id: "finished", label: "Completados" },
		{ id: "cancelled", label: "Cancelados" },
		{ id: "error", label: "Error" },
	];
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
					Cancelar cola activa
				</Button>
			{/if}
			<Button variant="outline" size="sm" href="/sign">Ir a Firmar</Button>
		</div>
	</div>

	<div class="flex flex-wrap items-center gap-2">
		{#each filterButtons as fb}
			<Button
				type="button"
				variant={filter === fb.id ? "default" : "outline"}
				size="sm"
				class="h-8 text-xs"
				onclick={() => {
					filter = fb.id;
				}}
			>
				{fb.label}
			</Button>
		{/each}
	</div>

	<div class="flex flex-wrap gap-2">
		<Button
			type="button"
			variant="secondary"
			size="sm"
			disabled={terminalCount === 0}
			onclick={() => void confirmClearFinished()}
		>
			Quitar finalizados ({terminalCount})
		</Button>
		<Button type="button" variant="destructive" size="sm" onclick={() => void confirmClearAll()}>
			Vaciar historial
		</Button>
	</div>

	<Card.Root>
		<Card.Header class="pb-2">
			<Card.Title class="text-base">Historial de lotes</Card.Title>
			<Card.Description>
				Misma lista que en Firmar: incluye preparación, firma en curso y resultados. Se guarda en el perfil de
				datos de NexoSign para poder revisarlo tras cerrar la ventana.
			</Card.Description>
		</Card.Header>
		<Card.Content class="pt-0">
			{#if filteredItems.length === 0}
				<p class="text-muted-foreground py-8 text-center text-sm">
					{filter === "all"
						? "Aún no hay lotes. Cuando firmes desde Firmar, aparecerán aquí."
						: "Nada que mostrar con este filtro."}
				</p>
			{:else}
				<ScrollArea.Root class="h-[min(60vh,520px)] pr-3">
					<div class="space-y-2">
						{#each filteredItems as q (q.jobId)}
							<div
								class={cn(
									"bg-muted/25 border-border/70 flex flex-wrap items-start justify-between gap-3 rounded-lg border px-3 py-2.5 text-sm",
									batchQueue.activeBatchJobId === q.jobId &&
										batchQueueHasActiveWork &&
										"ring-primary/40 ring-2",
								)}
							>
								<div class="min-w-0 flex-1 space-y-1">
									<p class="truncate font-mono text-xs" title={q.jobId}>{q.jobId}</p>
									<p class="text-muted-foreground truncate text-xs">{q.label}</p>
									<p class="text-muted-foreground text-[11px]">{formatWhen(q)}</p>
								</div>
								<div class="flex shrink-0 items-center gap-2">
									<Badge variant="secondary" class="text-[10px]">{statusLabel(q.status)}</Badge>
									<span class="text-muted-foreground tabular-nums text-[11px]">{q.progressPct}%</span>
									<Button
										type="button"
										variant="ghost"
										size="icon"
										class="size-8 shrink-0"
										title="Eliminar esta entrada"
										onclick={() => removeOne(q.jobId)}
									>
										<Trash2Icon class="size-4" />
									</Button>
								</div>
							</div>
						{/each}
					</div>
				</ScrollArea.Root>
			{/if}
		</Card.Content>
	</Card.Root>

	<p class="text-muted-foreground max-w-2xl text-xs leading-relaxed">
		Los trabajos «en curso» de una sesión anterior se marcan como error al volver a abrir la app (no había forma de
		seguirlos). Puedes borrar fila a fila o usar <strong class="text-foreground">Quitar finalizados</strong> para
		dejar solo activos e incompletos raros.
	</p>
</div>
