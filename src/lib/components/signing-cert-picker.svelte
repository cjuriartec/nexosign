<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert/index.js";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import {
		DEDUPED_WIN_MY_FOOTNOTE,
		emptySigningCertsHelp,
		emptySigningCertsHelpBrief,
		signingCertSourceLabel,
		signingCertSourceSubtitle,
	} from "$lib/tauri/pkcs11-ux";
	import { getHumanNameFromDn, extractDniFromDn } from "$lib/signature-appearance";
	import { cn } from "$lib/utils.js";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
	import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
	import IdCardIcon from "@lucide/svelte/icons/id-card";

	interface Props {
		certs: SigningCertSummary[];
		certId?: string;
		busy?: boolean;
		slotsWithToken?: number;
		/** `brief` para páginas con poco espacio (Certificados). */
		helpVariant?: "full" | "brief";
		showDedupeNote?: boolean;
		onRefresh?: () => void | Promise<void>;
		onResetReader?: () => void | Promise<void>;
		/** Asistente Firmar: menos padding y sin fila extra de acciones. */
		compact?: boolean;
		/** Mensaje extra cuando la lista está vacía (política chip vs MY). */
		contextHint?: string | null;
		class?: string;
	}

	let {
		certs,
		certId = $bindable(""),
		busy = false,
		slotsWithToken = 0,
		helpVariant = "full",
		showDedupeNote = true,
		onRefresh,
		onResetReader,
		compact = false,
		contextHint = null,
		class: className,
	}: Props = $props();

	const emptyHelp = $derived(
		helpVariant === "brief"
			? emptySigningCertsHelpBrief(slotsWithToken)
			: emptySigningCertsHelp(slotsWithToken),
	);
</script>

<div
	class={cn(
		compact ? "flex min-h-0 flex-1 flex-col gap-2" : "space-y-3",
		className,
	)}
>
	{#if onRefresh || onResetReader}
		<div class="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
			{#if onResetReader}
				<Button
					type="button"
					variant="ghost"
					size="sm"
					class="text-muted-foreground h-8 px-2 text-xs"
					disabled={busy}
					title="Cierra la conexión con el lector y vuelve a detectar la tarjeta"
					onclick={() => void onResetReader()}
				>
					Reconectar lector
				</Button>
			{/if}
			{#if onRefresh}
				<Button
					type="button"
					variant="outline"
					size="sm"
					class="h-8 gap-1.5 px-2.5 text-xs"
					disabled={busy}
					title="Actualizar certificados disponibles"
					onclick={() => void onRefresh()}
				>
					<RefreshCwIcon
						class={cn("size-3.5", busy && "animate-spin")}
						aria-hidden="true"
					/>
					{busy ? "Actualizando…" : "Actualizar"}
				</Button>
			{/if}
		</div>
	{:else if showDedupeNote}
		<p class="text-muted-foreground max-w-prose shrink-0 text-xs leading-snug">{DEDUPED_WIN_MY_FOOTNOTE}</p>
	{/if}

	{#if certs.length === 0}
		<Alert
			variant={slotsWithToken <= 0 ? "destructive" : "default"}
			class={cn("text-left", compact && "py-2")}
		>
			<TriangleAlertIcon class="size-4" />
			<AlertTitle class="text-sm">{emptyHelp.title}</AlertTitle>
			<AlertDescription class="space-y-1.5 text-xs leading-snug">
				<p>{emptyHelp.description}</p>
				{#if contextHint}
					<p class="text-muted-foreground">{contextHint}</p>
				{/if}
			</AlertDescription>
		</Alert>
	{:else}
		<div
			class={cn(
				compact ? "min-h-0 flex-1 space-y-1.5 overflow-y-auto pr-0.5" : "space-y-2",
			)}
			role="radiogroup"
			aria-label="Certificado de firma"
		>
			{#each certs as c (c.id_hex)}
				{@const selected = certId === c.id_hex}
				{@const name = getHumanNameFromDn(c.subject_dn) || c.label || "(sin etiqueta)"}
				{@const dni = extractDniFromDn(c.subject_dn)}
				{@const subtitle = signingCertSourceSubtitle(c.source, c.win_my_key_binding)}
				<button
					type="button"
					role="radio"
					aria-checked={selected}
					class={cn(
						"flex w-full items-center gap-2.5 rounded-lg border text-left transition-colors",
						compact ? "px-2.5 py-2" : "items-start gap-3 px-3 py-3",
						selected
							? "border-primary bg-primary/5 ring-primary/30 ring-2"
							: "border-border/80 bg-card hover:bg-muted/40",
					)}
					onclick={() => {
						certId = c.id_hex;
					}}
				>
					<span
						class={cn(
							"flex shrink-0 items-center justify-center rounded-full",
							compact ? "size-8" : "mt-0.5 size-9",
							selected ? "bg-primary/15 text-primary" : "bg-muted text-muted-foreground",
						)}
						aria-hidden="true"
					>
						<IdCardIcon class={compact ? "size-3.5" : "size-4"} />
					</span>
					<span class="min-w-0 flex-1">
						<span class="flex flex-wrap items-center gap-1.5">
							<span class="text-sm font-semibold leading-tight">
								{name}
							</span>
							<Badge variant="secondary" class="h-5 px-1.5 text-[10px] font-normal">
								{signingCertSourceLabel(c.source)}
							</Badge>
						</span>
						{#if dni}
							<span class="text-muted-foreground block text-[11px] tabular-nums leading-tight">{dni}</span>
						{/if}
						{#if subtitle}
							<span class="text-muted-foreground block text-[10px] leading-tight">{subtitle}</span>
						{/if}
					</span>
					{#if selected}
						<CircleCheckIcon
							class={cn("text-primary shrink-0", compact ? "size-4" : "mt-1 size-5")}
							aria-hidden="true"
						/>
					{/if}
				</button>
			{/each}
		</div>
	{/if}
</div>
