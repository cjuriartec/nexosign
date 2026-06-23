<script lang="ts">
	import * as Dialog from "$lib/components/ui/dialog/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import { SIG_GRID_COLS } from "$lib/sign/constants";
	import {
		signaturePlacementCellNumber,
		saveSignaturePlacement,
	} from "$lib/sign/signature-placement-prefs";
	import { cn } from "$lib/utils.js";
	import PenLineIcon from "@lucide/svelte/icons/pen-line";
	import LayoutGridIcon from "@lucide/svelte/icons/layout-grid";

	interface Props {
		sigGridCol?: number;
		sigGridRow?: number;
		disabled?: boolean;
		class?: string;
	}

	let {
		sigGridCol = $bindable(1),
		sigGridRow = $bindable(4),
		disabled = false,
		class: className,
	}: Props = $props();

	let open = $state(false);
	const cellNumber = $derived(signaturePlacementCellNumber(sigGridCol, sigGridRow));

	function selectCell(col: number, row: number) {
		sigGridCol = col;
		sigGridRow = row;
		saveSignaturePlacement({ col, row });
	}
</script>

<div class={cn("shrink-0", className)}>
	<button
		type="button"
		class="border-border/60 hover:bg-muted/40 flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left transition-colors disabled:opacity-50"
		disabled={disabled}
		onclick={() => {
			open = true;
		}}
	>
		<PenLineIcon class="text-muted-foreground size-3.5 shrink-0" aria-hidden="true" />
		<span class="min-w-0 flex-1 text-xs">
			<span class="font-medium">Ubicación del sello</span>
			<span class="text-muted-foreground"> · casilla {cellNumber}</span>
		</span>
		<Badge variant="outline" class="h-5 shrink-0 font-mono text-[10px] tabular-nums">
			{sigGridCol + 1}·{sigGridRow + 1}
		</Badge>
		<LayoutGridIcon class="text-muted-foreground size-3.5 shrink-0" aria-hidden="true" />
	</button>

	<Dialog.Root bind:open>
		<Dialog.Content class="sm:max-w-xs">
			<Dialog.Header>
				<Dialog.Title>Ubicación del sello</Dialog.Title>
				<Dialog.Description>Primera página del PDF · rejilla 3×5</Dialog.Description>
			</Dialog.Header>

			<div class="flex flex-col items-center gap-3 py-1">
				<Badge variant="secondary" class="h-6 px-2.5 font-mono text-xs tabular-nums">
					Casilla {cellNumber}
				</Badge>
				<div
					class="overflow-hidden rounded-xl border border-border bg-linear-to-b from-muted/30 to-muted/10 p-3 shadow-inner"
				>
					{#each [0, 1, 2, 3, 4] as row}
						<div class="flex gap-1.5 pb-1.5 last:pb-0">
							{#each [0, 1, 2] as col}
								<button
									type="button"
									class={cn(
										"flex size-10 shrink-0 items-center justify-center rounded-lg border text-xs font-semibold transition-all sm:size-11",
										sigGridCol === col && sigGridRow === row
											? "border-primary bg-primary text-primary-foreground shadow-md"
											: "border-border/70 bg-background text-muted-foreground hover:border-primary/30 hover:bg-muted/60",
									)}
									aria-label="Casilla {row * SIG_GRID_COLS + col + 1}"
									aria-pressed={sigGridCol === col && sigGridRow === row}
									onclick={() => selectCell(col, row)}
								>
									{row * SIG_GRID_COLS + col + 1}
								</button>
							{/each}
						</div>
					{/each}
				</div>
			</div>

			<Dialog.Footer>
				<Button type="button" class="w-full sm:w-auto" onclick={() => (open = false)}>
					Listo
				</Button>
			</Dialog.Footer>
		</Dialog.Content>
	</Dialog.Root>
</div>
