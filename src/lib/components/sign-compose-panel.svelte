<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import SigningCertPicker from "$lib/components/signing-cert-picker.svelte";
	import SignPlacementModal from "$lib/components/sign-placement-modal.svelte";
	import SignPinModal from "$lib/components/sign-pin-modal.svelte";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import { pdfBasenameFromPath } from "$lib/sign/path-util";
	import { cn } from "$lib/utils.js";
	import FileStackIcon from "@lucide/svelte/icons/files";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import UploadIcon from "@lucide/svelte/icons/upload";
	import KeyRoundIcon from "@lucide/svelte/icons/key-round";

	interface Props {
		paths: string[];
		sourceMode: "files" | "folder" | null;
		outputDirForJob: string | null;
		busy?: boolean;
		dropHover?: boolean;
		certs: SigningCertSummary[];
		certId?: string;
		slotsWithTokenCount?: number;
		listContextHint?: string | null;
		showPinProbe?: boolean;
		probePin?: string;
		pinProbeBusy?: boolean;
		pinRequired?: boolean;
		pin?: string;
		pinError?: string | null;
		pinVisible?: boolean;
		pinModalOpen?: boolean;
		sigGridCol?: number;
		sigGridRow?: number;
		submitInFlight?: boolean;
		signLabel?: string;
		onBrowse?: () => void;
		onBrowseFolder?: () => void;
		onClearPaths?: () => void;
		onRemoveAt?: (index: number) => void;
		onRefreshCerts?: () => void;
		onResetReader?: () => void;
		onTryListWithPin?: () => void;
		onSubmit?: () => void;
		class?: string;
	}

	let {
		paths,
		sourceMode,
		outputDirForJob,
		busy = false,
		dropHover = false,
		certs,
		certId = $bindable(""),
		slotsWithTokenCount = 0,
		listContextHint = null,
		showPinProbe = false,
		probePin = $bindable(""),
		pinProbeBusy = false,
		pinRequired = false,
		pin = $bindable(""),
		pinError = $bindable(null),
		pinVisible = $bindable(false),
		pinModalOpen = $bindable(false),
		sigGridCol = $bindable(1),
		sigGridRow = $bindable(4),
		submitInFlight = false,
		signLabel = "Firmar",
		onBrowse,
		onBrowseFolder,
		onClearPaths,
		onRemoveAt,
		onRefreshCerts,
		onResetReader,
		onTryListWithPin,
		onSubmit,
		class: className,
	}: Props = $props();

	const visiblePaths = $derived(paths.slice(0, 6));
	const hiddenCount = $derived(Math.max(0, paths.length - visiblePaths.length));
</script>

<div class={cn("flex min-h-0 flex-1 flex-col gap-3", className)}>
	<!-- Zona de carga -->
	<div
		class={cn(
			"relative shrink-0 overflow-hidden rounded-xl border-2 border-dashed transition-colors",
			dropHover
				? "border-primary bg-primary/10"
				: paths.length === 0
					? "border-border/80 bg-muted/15 hover:border-primary/35"
					: "border-border/60 bg-muted/10",
		)}
	>
		{#if dropHover}
			<div
				class="bg-primary/5 pointer-events-none absolute inset-0 z-10 flex flex-col items-center justify-center gap-2"
			>
				<UploadIcon class="text-primary size-8" aria-hidden="true" />
				<p class="text-primary text-sm font-medium">Suelta PDF o carpeta</p>
			</div>
		{/if}

		<div class="flex flex-col gap-3 p-4 sm:flex-row sm:items-center sm:justify-between">
			<div class="min-w-0 space-y-1">
				<p class="text-sm font-medium">
					{#if paths.length === 0}
						Arrastra PDF o carpetas aquí
					{:else}
						{paths.length} PDF listo{paths.length === 1 ? "" : "s"}
					{/if}
				</p>
				<p class="text-muted-foreground text-xs leading-snug">
					{#if paths.length === 0}
						También puedes elegir archivos o una carpeta. Detectamos automáticamente el tipo.
					{:else if sourceMode === "folder" && outputDirForJob}
						Salida en <span class="font-mono">{pdfBasenameFromPath(outputDirForJob)}</span>
					{:else}
						Se guardará junto al original como <code class="font-mono text-[10px]">*_firmado.pdf</code>
					{/if}
				</p>
			</div>
			<div class="flex shrink-0 flex-wrap gap-2">
				<Button type="button" size="sm" variant="secondary" class="gap-1.5" disabled={busy} onclick={() => onBrowse?.()}>
					<FileStackIcon class="size-4" aria-hidden="true" />
					Archivos
				</Button>
				<Button type="button" size="sm" variant="outline" class="gap-1.5" disabled={busy} onclick={() => onBrowseFolder?.()}>
					<FolderOpenIcon class="size-4" aria-hidden="true" />
					Carpeta
				</Button>
				{#if paths.length > 0}
					<Button
						type="button"
						size="sm"
						variant="ghost"
						class="text-destructive"
						disabled={busy}
						onclick={() => onClearPaths?.()}
					>
						Limpiar
					</Button>
				{/if}
			</div>
		</div>
	</div>

	{#if paths.length > 0}
		<ul class="border-border/70 bg-muted/10 scrollbar-subtle max-h-28 shrink-0 space-y-0.5 overflow-y-auto rounded-lg border px-2 py-1.5">
			{#each visiblePaths as p, i (p)}
				<li class="flex items-center gap-2 py-0.5">
					<span class="text-muted-foreground w-5 shrink-0 text-right text-[10px] tabular-nums">{i + 1}</span>
					<span class="min-w-0 flex-1 truncate font-mono text-[11px]" title={p}>{pdfBasenameFromPath(p)}</span>
					<Button
						type="button"
						variant="ghost"
						size="icon-xs"
						class="text-destructive shrink-0"
						aria-label="Quitar"
						onclick={() => onRemoveAt?.(i)}
					>
						<Trash2Icon class="size-3.5" />
					</Button>
				</li>
			{/each}
			{#if hiddenCount > 0}
				<li class="text-muted-foreground px-7 py-0.5 text-[11px]">… y {hiddenCount} más</li>
			{/if}
		</ul>
	{/if}

	<!-- Certificado -->
	<div class="min-h-0 shrink-0 space-y-2">
		{#if showPinProbe}
			<div class="border-border/70 bg-muted/15 space-y-2 rounded-lg border border-dashed px-2.5 py-2">
				<div class="flex items-start gap-2">
					<KeyRoundIcon class="text-muted-foreground mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
					<p class="text-muted-foreground text-[11px] leading-snug">
						Introduce el PIN del DNIe para detectar el certificado del chip.
					</p>
				</div>
				<div class="flex flex-wrap items-end gap-2">
					<div class="min-w-[7rem] flex-1 space-y-1">
						<Label for="sign-pin-probe" class="text-xs">PIN</Label>
						<Input
							id="sign-pin-probe"
							type="password"
							autocomplete="off"
							placeholder="••••"
							class="h-8"
							bind:value={probePin}
							disabled={pinProbeBusy || busy}
							onkeydown={(e) => {
								if (e.key === "Enter" && probePin.trim()) void onTryListWithPin?.();
							}}
						/>
					</div>
					<Button
						variant="secondary"
						size="sm"
						class="h-8 shrink-0"
						disabled={pinProbeBusy || busy || !probePin.trim()}
						onclick={() => onTryListWithPin?.()}
					>
						{pinProbeBusy ? "Leyendo…" : "Probar PIN"}
					</Button>
				</div>
			</div>
		{/if}

		<SigningCertPicker
			{certs}
			bind:certId
			busy={busy || pinProbeBusy}
			slotsWithToken={slotsWithTokenCount}
			contextHint={listContextHint}
			helpVariant="brief"
			showDedupeNote={false}
			compact
			onRefresh={() => onRefreshCerts?.()}
			onResetReader={() => onResetReader?.()}
		/>
	</div>

	{#if pinRequired && paths.length > 0}
		<SignPinModal
			bind:open={pinModalOpen}
			bind:pin
			bind:pinError
			bind:pinVisible
			disabled={busy}
			{submitInFlight}
			{signLabel}
			onConfirm={() => onSubmit?.()}
		/>
	{/if}

	<SignPlacementModal bind:sigGridCol bind:sigGridRow disabled={busy || submitInFlight} />
</div>
