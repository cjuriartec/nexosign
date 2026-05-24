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
		/** Asistente Firmar: menos padding y sin fila extra de acciones. */
		compact?: boolean;
	}

	let {
		certs,
		certId = $bindable(""),
		busy = false,
		slotsWithToken = 0,
		helpVariant = "full",
		showDedupeNote = true,
		onRefresh,
		compact = false,
	}: Props = $props();

	const emptyHelp = $derived(
		helpVariant === "brief"
			? emptySigningCertsHelpBrief(slotsWithToken)
			: emptySigningCertsHelp(slotsWithToken),
	);
</script>

<div class={cn(compact ? "space-y-2" : "space-y-3")}>
	{#if onRefresh}
		<div class="flex justify-end">
			<Button
				type="button"
				variant="outline"
				size="sm"
				class="shrink-0 gap-1.5"
				disabled={busy}
				onclick={() => void onRefresh()}
			>
				<RefreshCwIcon class="size-4 opacity-80" aria-hidden="true" />
				Actualizar
			</Button>
		</div>
	{:else if showDedupeNote}
		<p class="text-muted-foreground max-w-prose text-xs leading-snug">{DEDUPED_WIN_MY_FOOTNOTE}</p>
	{/if}

	{#if certs.length === 0}
		<Alert
			variant={slotsWithToken <= 0 ? "destructive" : "default"}
			class={cn("text-left", compact && "py-2")}
		>
			<TriangleAlertIcon class="size-4" />
			<AlertTitle class="text-sm">{emptyHelp.title}</AlertTitle>
			{#if helpVariant === "full"}
				<AlertDescription class="text-xs leading-snug">{emptyHelp.description}</AlertDescription>
			{:else}
				<AlertDescription class="text-xs">{emptyHelp.description}</AlertDescription>
			{/if}
		</Alert>
	{:else}
		<div class={cn(compact ? "space-y-1.5" : "space-y-2")} role="radiogroup" aria-label="Certificado de firma">
			{#each certs as c (c.id_hex)}
				{@const selected = certId === c.id_hex}
				{@const name = getHumanNameFromDn(c.subject_dn) || c.label || "(sin etiqueta)"}
				{@const dni = extractDniFromDn(c.subject_dn)}
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
