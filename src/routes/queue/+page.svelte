<script lang="ts">
	import { goto } from "$app/navigation";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import * as ScrollArea from "$lib/components/ui/scroll-area/index.js";
	import { cancelActiveBatchJob } from "$lib/batch/cancel-active-batch";
	import { ask } from "@tauri-apps/plugin-dialog";
	import { toast } from "svelte-sonner";
	import Loader2Icon from "@lucide/svelte/icons/loader-2";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import * as Progress from "$lib/components/ui/progress/index.js";
	import { cn } from "$lib/utils.js";
	import {
		ACTIVE_BATCH_STATUSES,
		batchQueue,
		clearBatchQueue,
		clearTerminalBatchQueueItems,
		intentQueue,
		removeBatchQueueItem,
		removeIntentQueueItem,
		computeBatchQueueHasActiveWork,
		TERMINAL_BATCH_STATUSES,
		type BatchQueueItem,
		type BatchQueueStatus,
		type IntentQueueItem,
	} from "$lib/stores/batch-queue.svelte";

	type QueueFilter = "all" | "intents" | "active" | "finished" | "cancelled" | "error";

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

	const filteredJobs = $derived.by(() => {
		const list = batchQueue.items;
		if (filter === "all") return list;
		if (filter === "active") {
			return list.filter((q) =>
				["preparing", "queued", "running", "cancelling"].includes(q.status),
			);
		}
		if (filter === "finished") return list.filter((q) => q.status === "finished");
		if (filter === "cancelled") return list.filter((q) => q.status === "cancelled");
		if (filter === "error") return list.filter((q) => q.status === "error");
		return list;
	});

	const terminalCount = $derived(
		batchQueue.items.filter((q) => TERMINAL_BATCH_STATUSES.includes(q.status)).length,
	);

	const showIntentBlock = $derived(filter === "all" || filter === "intents");
	const showJobBlock = $derived(filter !== "intents");

	function statusLabel(s: BatchQueueStatus): string {
		switch (s) {
			case "preparing":
				return "Preparando";
			case "queued":
				return "En cola";
			case "running":
				return "En curso";
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

	function shortJobId(id: string): string {
		if (id.length <= 18) return id;
		return `${id.slice(0, 10)}…${id.slice(-6)}`;
	}

	function badgeVariantForStatus(
		s: BatchQueueStatus,
	): "default" | "secondary" | "destructive" | "outline" {
		switch (s) {
			case "running":
			case "queued":
			case "preparing":
				return "default";
			case "cancelling":
				return "secondary";
			case "error":
				return "destructive";
			case "finished":
				return "outline";
			default:
				return "outline";
		}
	}

	function activeStepHint(s: BatchQueueStatus): string {
		switch (s) {
			case "running":
				return "Procesando PDFs…";
			case "queued":
				return "Esperando turno en la cola…";
			case "preparing":
				return "Preparando el lote…";
			case "cancelling":
				return "Deteniendo la firma…";
			default:
				return "";
		}
	}

	function formatJobWhen(it: BatchQueueItem): string {
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

	function formatIntentWhen(it: IntentQueueItem): string {
		try {
			return new Intl.DateTimeFormat("es", {
				dateStyle: "short",
				timeStyle: "short",
			}).format(new Date(it.createdAt));
		} catch {
			return "—";
		}
	}

	async function confirmClearAll(): Promise<void> {
		const ok = await ask("¿Vaciar todo el historial?", { title: "Colas", kind: "warning" });
		if (!ok) return;
		clearBatchQueue();
		toast.success("Listo.");
	}

	async function confirmClearFinished(): Promise<void> {
		const n = terminalCount;
		if (n === 0) return;
		const ok = await ask(`¿Quitar ${n} entrada(s) ya terminadas?`, {
			title: "Colas",
			kind: "info",
		});
		if (!ok) return;
		clearTerminalBatchQueueItems();
		toast.success("Listo.");
	}

	function removeJob(jobId: string): void {
		removeBatchQueueItem(jobId);
	}

	function removeIntent(requestId: string): void {
		removeIntentQueueItem(requestId);
	}

	function continueIntent(requestId: string): void {
		void goto(`/sign?intent=${encodeURIComponent(requestId)}`);
	}

	const filterButtons: { id: QueueFilter; label: string }[] = [
		{ id: "all", label: "Todos" },
		{ id: "intents", label: "Pendientes" },
		{ id: "active", label: "En curso" },
		{ id: "finished", label: "Completados" },
		{ id: "cancelled", label: "Cancelados" },
		{ id: "error", label: "Errores" },
	];

	const listEmpty = $derived.by(() => {
		const ni = showIntentBlock ? intentQueue.items.length : 0;
		const nj = showJobBlock ? filteredJobs.length : 0;
		return ni === 0 && nj === 0;
	});
</script>

<svelte:head>
	<title>Colas — NexoSign</title>
</svelte:head>

<div class="space-y-3 pb-6">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<h1 class="text-2xl font-semibold tracking-tight">Colas</h1>
		<div class="flex flex-wrap items-center gap-2">
			{#if canCancelActiveJob}
				<Button type="button" variant="outline" size="sm" onclick={() => void cancelActiveBatchJob()}>
					Cancelar
				</Button>
			{/if}
			<Button variant="outline" size="sm" href="/sign">Firmar</Button>
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
			Quitar terminados ({terminalCount})
		</Button>
		<Button type="button" variant="destructive" size="sm" onclick={() => void confirmClearAll()}>
			Vaciar todo
		</Button>
	</div>

	<Card.Root>
		<Card.Content class="pt-6">
			{#if listEmpty}
				<p class="text-muted-foreground py-6 text-center text-sm">
					{filter === "all" ? "Sin entradas." : "Sin resultados."}
				</p>
			{:else}
				<ScrollArea.Root class="max-h-[min(60vh,560px)] pr-3">
					<div class="space-y-6">
						{#if showIntentBlock && intentQueue.items.length > 0}
							<div class="space-y-2">
								<p class="text-muted-foreground text-xs font-medium uppercase tracking-wide">Pendientes</p>
								{#each intentQueue.items as it (it.requestId)}
									<div
										class={cn(
											"bg-muted/25 border-border/70 flex flex-wrap items-start justify-between gap-3 rounded-lg border px-3 py-2.5 text-sm",
											intentQueue.activeRequestId === it.requestId && "ring-primary/40 ring-2",
										)}
									>
										<div class="min-w-0 flex-1 space-y-1">
											<p class="truncate font-mono text-xs" title={it.requestId}>{it.requestId}</p>
											<p class="text-muted-foreground truncate text-xs">{it.label}</p>
											<p class="text-muted-foreground text-[11px]">{formatIntentWhen(it)}</p>
										</div>
										<div class="flex shrink-0 flex-wrap items-center justify-end gap-2">
											<Badge variant="outline" class="text-[10px]">Pendientes</Badge>
											<Button
												type="button"
												size="sm"
												class="h-8 text-xs"
												onclick={() => continueIntent(it.requestId)}
											>
												Continuar
											</Button>
											<Button
												type="button"
												variant="ghost"
												size="icon"
												class="size-8 shrink-0"
												title="Quitar"
												onclick={() => removeIntent(it.requestId)}
											>
												<Trash2Icon class="size-4" />
											</Button>
										</div>
									</div>
								{/each}
							</div>
						{/if}

						{#if showJobBlock && filteredJobs.length > 0}
							<div class="space-y-2">
								{#if showIntentBlock && intentQueue.items.length > 0}
									<p class="text-muted-foreground text-xs font-medium uppercase tracking-wide">Firma</p>
								{/if}
								{#each filteredJobs as q (q.jobId)}
									{@const isActive = ACTIVE_BATCH_STATUSES.includes(q.status)}
									<div
										class={cn(
											"overflow-hidden rounded-xl border text-sm transition-colors",
											isActive
												? "border-primary/40 bg-linear-to-b from-primary/[0.07] to-transparent"
												: "bg-muted/25 border-border/70",
											batchQueue.activeBatchJobId === q.jobId &&
												batchQueueHasActiveWork &&
												"ring-primary/30 ring-2 ring-offset-2 ring-offset-background",
										)}
									>
										<div class="flex flex-wrap items-start gap-3 px-3 py-3">
											<div class="min-w-0 flex-1 space-y-1">
												<div class="flex flex-wrap items-center gap-2">
													<p class="truncate font-medium leading-snug">{q.label}</p>
													{#if q.status === "running"}
														<Loader2Icon
															class="text-primary size-3.5 shrink-0 animate-spin"
															aria-hidden="true"
														/>
													{/if}
												</div>
												<p
													class="text-muted-foreground truncate font-mono text-[11px]"
													title={q.jobId}
												>
													{shortJobId(q.jobId)}
												</p>
												<p class="text-muted-foreground text-[11px]">{formatJobWhen(q)}</p>
											</div>
											<div class="flex shrink-0 items-start gap-2">
												<div class="flex flex-col items-end gap-1">
													<Badge
														variant={badgeVariantForStatus(q.status)}
														class="text-[10px] font-medium"
													>
														{statusLabel(q.status)}
													</Badge>
													<span
														class={cn(
															"tabular-nums text-[11px]",
															isActive ? "text-primary font-semibold" : "text-muted-foreground",
														)}
													>
														{q.progressPct}%
													</span>
												</div>
												<Button
													type="button"
													variant="ghost"
													size="icon"
													class="text-muted-foreground hover:text-foreground size-8 shrink-0"
													title="Quitar"
													onclick={() => removeJob(q.jobId)}
												>
													<Trash2Icon class="size-4" />
												</Button>
											</div>
										</div>
										{#if isActive}
											<div class="border-border/50 bg-background/40 border-t px-3 pb-3 pt-2">
												<Progress.Root class="h-2" value={q.progressPct} max={100} />
												<p class="text-muted-foreground mt-2 text-[11px] leading-snug">
													{activeStepHint(q.status)}
												</p>
											</div>
										{/if}
									</div>
								{/each}
							</div>
						{/if}
					</div>
				</ScrollArea.Root>
			{/if}
		</Card.Content>
	</Card.Root>
</div>
