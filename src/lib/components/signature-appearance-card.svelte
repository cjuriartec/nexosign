<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { cn } from "$lib/utils.js";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import GripVerticalIcon from "@lucide/svelte/icons/grip-vertical";
	import XIcon from "@lucide/svelte/icons/x";
	import ImagePlusIcon from "@lucide/svelte/icons/image-plus";
	import RotateCcwIcon from "@lucide/svelte/icons/rotate-ccw";
	import ArrowDownToLineIcon from "@lucide/svelte/icons/arrow-down-to-line";
	import {
		PALETTE_ITEMS,
		type SigPart,
		type SignatureAppearanceState,
		clonePart,
		defaultSignatureAppearance,
		loadSignatureAppearance,
		saveSignatureAppearance,
		resolvePartsToPreviewLines,
		previewImageSrc,
		tokenLabel,
	} from "$lib/signature-appearance";

	let {
		certs = [],
		compact = false,
		/** Oculta el título cuando la página ya lleva cabecera de sección. */
		hideHeader = false,
	}: { certs: SigningCertSummary[]; compact?: boolean; hideHeader?: boolean } = $props();

	let appearance = $state<SignatureAppearanceState>(defaultSignatureAppearance());
	let fileInput = $state<HTMLInputElement | null>(null);
	let listEl = $state<HTMLElement | null>(null);

	type DragState =
		| { source: "palette"; part: SigPart; label: string; pointerId: number; armed: boolean; startX: number; startY: number }
		| { source: "editor"; fromIndex: number; label: string; pointerId: number; armed: boolean; startX: number; startY: number };

	let drag = $state<DragState | null>(null);
	let ghostX = $state(0);
	let ghostY = $state(0);
	let dropAt = $state<number | null>(null);
	const DRAG_THRESHOLD = 4;

	const previewCert = $derived(certs[0] ?? null);
	const previewLines = $derived(resolvePartsToPreviewLines(appearance.parts, previewCert));
	const imgSrc = $derived(previewImageSrc(appearance));

	function persist() {
		saveSignatureAppearance(appearance);
	}

	onMount(() => {
		appearance = loadSignatureAppearance();
		return () => cleanupGlobalListeners();
	});

	function resetAll() {
		appearance = defaultSignatureAppearance();
		persist();
		toast.message("Diseño restablecido");
	}

	function appendPart(part: SigPart) {
		appearance = { ...appearance, parts: [...appearance.parts, clonePart(part)] };
		persist();
	}

	function removeAt(i: number) {
		appearance = { ...appearance, parts: appearance.parts.filter((_, idx) => idx !== i) };
		persist();
	}

	function movePart(i: number, delta: number) {
		const j = i + delta;
		if (j < 0 || j >= appearance.parts.length) return;
		const parts = [...appearance.parts];
		[parts[i], parts[j]] = [parts[j], parts[i]];
		appearance = { ...appearance, parts };
		persist();
	}

	function updateText(i: number, value: string) {
		const parts = appearance.parts.map((p, idx) =>
			idx === i && p.kind === "text" ? { ...p, value } : p,
		);
		appearance = { ...appearance, parts };
		persist();
	}

	function reorderWheel(node: HTMLElement, index: number) {
		let i = index;
		const onWheel = (e: WheelEvent) => {
			e.preventDefault();
			e.stopPropagation();
			if (e.deltaY < 0) movePart(i, -1);
			else if (e.deltaY > 0) movePart(i, 1);
		};
		node.addEventListener("wheel", onWheel, { passive: false });
		return {
			update(newIndex: number) {
				i = newIndex;
			},
			destroy() {
				node.removeEventListener("wheel", onWheel);
			},
		};
	}

	function partLabel(part: SigPart): string {
		if (part.kind === "token") return tokenLabel(part.id);
		if (part.kind === "break") return "Salto";
		return part.value || "Texto";
	}

	function startPaletteDrag(e: PointerEvent, part: SigPart, label: string) {
		if (e.button !== 0) return;
		drag = {
			source: "palette",
			part: clonePart(part),
			label,
			pointerId: e.pointerId,
			armed: false,
			startX: e.clientX,
			startY: e.clientY,
		};
		ghostX = e.clientX;
		ghostY = e.clientY;
		attachGlobalListeners();
	}

	function startEditorDrag(e: PointerEvent, index: number) {
		if (e.button !== 0) return;
		const part = appearance.parts[index];
		if (!part) return;
		drag = {
			source: "editor",
			fromIndex: index,
			label: partLabel(part),
			pointerId: e.pointerId,
			armed: false,
			startX: e.clientX,
			startY: e.clientY,
		};
		ghostX = e.clientX;
		ghostY = e.clientY;
		attachGlobalListeners();
	}

	function attachGlobalListeners() {
		window.addEventListener("pointermove", onPointerMove, { passive: false });
		window.addEventListener("pointerup", onPointerUp);
		window.addEventListener("pointercancel", onPointerCancel);
		window.addEventListener("keydown", onKeyDown);
	}

	function cleanupGlobalListeners() {
		window.removeEventListener("pointermove", onPointerMove);
		window.removeEventListener("pointerup", onPointerUp);
		window.removeEventListener("pointercancel", onPointerCancel);
		window.removeEventListener("keydown", onKeyDown);
	}

	function onPointerMove(e: PointerEvent) {
		if (!drag) return;
		if (e.pointerId !== drag.pointerId) return;
		if (!drag.armed) {
			const dx = e.clientX - drag.startX;
			const dy = e.clientY - drag.startY;
			if (dx * dx + dy * dy < DRAG_THRESHOLD * DRAG_THRESHOLD) return;
			drag = { ...drag, armed: true } as DragState;
			document.body.classList.add("nexo-dnd-active");
		}
		e.preventDefault();
		ghostX = e.clientX;
		ghostY = e.clientY;
		dropAt = computeDropAt(e.clientX, e.clientY);
	}

	function onPointerUp(e: PointerEvent) {
		if (!drag) return;
		if (e.pointerId !== drag.pointerId) return;
		const armed = drag.armed;
		const target = dropAt;
		const current = drag;
		finishDrag();
		if (!armed) return;
		if (target === null) return;
		applyDrop(current, target);
	}

	function onPointerCancel() {
		finishDrag();
	}

	function onKeyDown(e: KeyboardEvent) {
		if (e.key === "Escape") finishDrag();
	}

	function finishDrag() {
		drag = null;
		dropAt = null;
		document.body.classList.remove("nexo-dnd-active");
		cleanupGlobalListeners();
	}

	function computeDropAt(x: number, y: number): number | null {
		if (!listEl) return null;
		const listRect = listEl.getBoundingClientRect();
		const SLACK = 80;
		if (
			x < listRect.left - SLACK ||
			x > listRect.right + SLACK ||
			y < listRect.top - SLACK ||
			y > listRect.bottom + SLACK
		) {
			return null;
		}
		const rows = Array.from(listEl.querySelectorAll<HTMLElement>("[data-row]"));
		if (rows.length === 0) return 0;
		for (let i = 0; i < rows.length; i++) {
			const r = rows[i].getBoundingClientRect();
			const mid = r.top + r.height / 2;
			if (y < mid) return i;
		}
		return rows.length;
	}

	function applyDrop(current: DragState, insertAt: number) {
		let parts = [...appearance.parts];
		if (current.source === "palette") {
			parts.splice(insertAt, 0, clonePart(current.part));
		} else {
			const from = current.fromIndex;
			if (from < 0 || from >= parts.length) return;
			if (insertAt === from || insertAt === from + 1) return;
			const moved = parts[from];
			if (!moved) return;
			parts = parts.filter((_, i) => i !== from);
			let to = insertAt;
			if (from < insertAt) to--;
			parts.splice(to, 0, moved);
		}
		appearance = { ...appearance, parts };
		persist();
	}

	function pickImage() {
		fileInput?.click();
	}

	function onImageFile(e: Event) {
		const input = e.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		input.value = "";
		if (!file) return;
		if (file.size > 1_800_000) {
			toast.error("Elige una imagen más pequeña.");
			return;
		}
		const reader = new FileReader();
		reader.onload = () => {
			const dataUrl = typeof reader.result === "string" ? reader.result : null;
			if (!dataUrl) return;
			appearance = {
				...appearance,
				imageMode: "custom",
				customImageDataUrl: dataUrl,
			};
			persist();
		};
		reader.readAsDataURL(file);
	}

	function useBundledImage() {
		appearance = {
			...appearance,
			imageMode: "bundled",
			customImageDataUrl: null,
		};
		persist();
	}

	const draggingEditorIndex = $derived(
		drag?.source === "editor" && drag.armed ? drag.fromIndex : null,
	);
	const isDragging = $derived(drag !== null && drag.armed);
</script>

<div class="@container w-full min-w-0 space-y-3">
	{#if !hideHeader}
		<header class="space-y-0.5">
			<h2 class="text-base font-semibold tracking-tight">Diseño del sello</h2>
			{#if !compact}
				<p class="text-muted-foreground text-sm leading-snug">
					Tres columnas cuando este bloque alcanza ~640px de ancho (con menú lateral, haz la ventana más ancha). Si no, se apilan: campos → orden → vista previa.
				</p>
			{/if}
		</header>
	{/if}

	<div
		class="grid grid-cols-1 gap-3 @min-[640px]:grid-cols-[minmax(0,1fr)_minmax(0,2fr)_minmax(0,2fr)] @min-[640px]:gap-3"
	>
		<!-- 1 · Chips -->
		<div
			class="border-border/50 bg-muted/15 flex flex-col gap-2 rounded-lg border p-3 @min-[640px]:max-h-[min(85vh,520px)] @min-[640px]:overflow-y-auto"
		>
			<div>
				<h3 class="text-sm font-medium">Campos</h3>
				{#if !compact}
					<p class="text-muted-foreground mt-0.5 text-xs leading-snug">
						Pulsa para añadir al final, o arrastra hacia «Orden».
					</p>
				{/if}
			</div>
			<div class="flex flex-col gap-1.5">
				{#each PALETTE_ITEMS as item}
					<button
						type="button"
						class="border-input bg-background hover:bg-accent hover:text-accent-foreground inline-flex w-full cursor-grab touch-none select-none items-center justify-start rounded-md border px-2.5 py-1.5 text-left text-sm font-medium shadow-sm transition-colors active:cursor-grabbing"
						onpointerdown={(e) => startPaletteDrag(e, item.part, item.label)}
						onclick={() => appendPart(item.part)}
					>
						{item.label}
					</button>
				{/each}
			</div>
		</div>

		<!-- 2 · Orden -->
		<div class="border-border/50 flex min-h-0 min-w-0 select-none flex-col gap-2 rounded-lg border p-3 @min-[640px]:max-h-[min(85vh,520px)]">
			<div class="shrink-0">
				<h3 class="text-sm font-medium">Orden del sello</h3>
				{#if !compact}
					<p class="text-muted-foreground mt-0.5 text-xs leading-snug">
						Arrastra desde el control izquierdo (n.º + ⋮⋮) o usa la rueda del ratón sobre él.
					</p>
				{/if}
			</div>

			<div class="min-h-0 flex-1 overflow-y-auto pr-0.5" bind:this={listEl}>
				{#if appearance.parts.length === 0}
					<div
						class={cn(
							"flex min-h-[120px] flex-col items-center justify-center gap-1 rounded-md border-2 border-dashed px-2 py-4 text-center transition-colors",
							isDragging && dropAt !== null
								? "border-primary bg-primary/10"
								: "border-muted-foreground/25 bg-muted/20",
						)}
						data-row
					>
						<p class="text-muted-foreground text-sm">Sin campos</p>
						{#if !compact}
							<p class="text-muted-foreground text-xs">Pulsa o arrastra desde la izquierda.</p>
						{/if}
					</div>
				{:else}
					<div class="flex flex-col gap-0">
						{#each appearance.parts as part, i}
							{@const showIndicator = isDragging && dropAt === i}
							{@const isOrigin = draggingEditorIndex === i}

							<div
								class={cn(
									"h-1 rounded-full transition-all duration-150",
									showIndicator ? "bg-primary my-1.5 h-1.5" : "my-0",
								)}
							></div>

							<div
								data-row
								class={cn(
									"border-input bg-card text-card-foreground my-1 flex items-center gap-1 rounded-lg border px-1.5 py-1 shadow-sm transition-opacity",
									isOrigin && "opacity-30",
								)}
							>
								<div
									class="text-muted-foreground hover:text-foreground border-border/60 bg-muted/50 flex shrink-0 cursor-grab touch-none select-none items-center gap-0.5 rounded-md border px-0.5 py-0.5 active:cursor-grabbing"
									role="button"
									tabindex="0"
									use:reorderWheel={i}
									aria-label="Orden: arrastra para mover, rueda del ratón para subir o bajar"
									title="Arrastra para reordenar · rueda: subir/bajar"
									onpointerdown={(e) => startEditorDrag(e, i)}
									onkeydown={(e) => {
										if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
										e.preventDefault();
										if (e.key === "ArrowUp") movePart(i, -1);
										else movePart(i, 1);
									}}
								>
									<span class="flex size-6 items-center justify-center text-[10px] font-semibold tabular-nums">
										{i + 1}
									</span>
									<GripVerticalIcon class="size-4 shrink-0" aria-hidden="true" />
								</div>

								<div class="min-w-0 flex-1">
									{#if part.kind === "token"}
										<p class="truncate text-sm font-medium leading-tight">{tokenLabel(part.id)}</p>
									{:else if part.kind === "break"}
										<div class="flex items-center gap-1">
											<ArrowDownToLineIcon class="text-muted-foreground size-3 shrink-0" />
											<p class="text-sm font-medium leading-tight">Salto</p>
										</div>
									{:else}
										<label class="sr-only" for="sig-text-{i}">Texto</label>
										<Input
											id="sig-text-{i}"
											class="h-7 select-text text-xs"
											value={part.value}
											placeholder="Texto…"
											oninput={(e) => updateText(i, (e.currentTarget as HTMLInputElement).value)}
										/>
									{/if}
								</div>

								<div class="flex shrink-0 items-center gap-0">
									<Button
										type="button"
										variant="ghost"
										size="icon"
										class="text-muted-foreground hover:text-destructive size-7"
										aria-label="Quitar"
										onclick={() => removeAt(i)}
									>
										<XIcon class="size-3.5" />
									</Button>
								</div>
							</div>
						{/each}

						<div
							class={cn(
								"h-1 rounded-full transition-all duration-150",
								isDragging && dropAt === appearance.parts.length ? "bg-primary my-1.5 h-1.5" : "my-0",
							)}
						></div>
					</div>
				{/if}
			</div>

			<Button type="button" variant="outline" size="sm" class="shrink-0 w-full gap-2" onclick={() => resetAll()}>
				<RotateCcwIcon class="size-3.5" />
				Restablecer
			</Button>
		</div>

		<!-- 3 · Vista previa -->
		<div
			class="border-border/50 bg-muted/10 flex flex-col gap-2 rounded-lg border p-3 @min-[640px]:max-h-[min(85vh,520px)] @min-[640px]:overflow-y-auto"
		>
			<div>
				<h3 class="text-sm font-medium">Vista previa</h3>
				{#if !compact}
					<p class="text-muted-foreground mt-0.5 text-xs leading-snug">Imagen y texto.</p>
				{/if}
			</div>

			<div class="flex flex-wrap gap-1.5">
				<Button type="button" variant="secondary" size="sm" class="min-w-0 flex-1 gap-1.5 @min-[640px]:flex-none" onclick={() => pickImage()}>
					<ImagePlusIcon class="size-4" />
					Imagen
				</Button>
				<Button type="button" variant="outline" size="sm" class="min-w-0 flex-1 @min-[640px]:flex-none" onclick={() => useBundledImage()}>
					Por defecto
				</Button>
				<input
					bind:this={fileInput}
					type="file"
					accept="image/png,image/jpeg,image/webp"
					class="sr-only"
					aria-hidden="true"
					onchange={(e) => onImageFile(e)}
				/>
			</div>

			<div class="mx-auto w-min min-w-[120px] overflow-hidden rounded-md border border-border bg-background shadow-sm ring-1 ring-black/5 dark:ring-white/10">
				<div class="flex justify-center pb-1">
					<img src={imgSrc} alt="" class="w-[120px] object-contain" />
				</div>
				<div class="px-2 pt-1 pb-2 text-justify">
					{#each previewLines as line}
						<p class="text-[9px] leading-tight text-foreground wrap-break-word">{line}</p>
					{/each}
				</div>
				{#if !previewCert}
					<p class="text-muted-foreground text-[9px] leading-tight">Sin DNIe ni tarjeta: vista de ejemplo.</p>
				{/if}
			</div>

		</div>
	</div>
</div>

{#if isDragging && drag}
	<div
		class="border-primary bg-card text-card-foreground pointer-events-none fixed z-60 -translate-x-1/2 -translate-y-1/2 rounded-md border px-2 py-1 text-xs font-medium shadow-lg"
		style="left: {ghostX}px; top: {ghostY}px;"
		aria-hidden="true"
	>
		{drag.label}
	</div>
{/if}

<style>
	:global(body.nexo-dnd-active) {
		cursor: grabbing !important;
		user-select: none;
	}
	:global(body.nexo-dnd-active *) {
		cursor: grabbing !important;
	}
</style>
