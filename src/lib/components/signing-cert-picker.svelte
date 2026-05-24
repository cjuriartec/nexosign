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
	}

	let {
		certs,
		certId = $bindable(""),
		busy = false,
		slotsWithToken = 0,
		helpVariant = "full",
		showDedupeNote = true,
		onRefresh,
	}: Props = $props();

	const emptyHelp = $derived(
		helpVariant === "brief"
			? emptySigningCertsHelpBrief(slotsWithToken)
			: emptySigningCertsHelp(slotsWithToken),
	);
</script>

<div class="space-y-3">
	{#if showDedupeNote || onRefresh}
	<div class="flex flex-wrap items-start justify-between gap-2">
		{#if showDedupeNote}
			<p class="text-muted-foreground max-w-prose text-xs leading-snug">{DEDUPED_WIN_MY_FOOTNOTE}</p>
		{/if}
		{#if onRefresh}
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
		{/if}
	</div>
	{/if}

	{#if certs.length === 0}
		<Alert variant={slotsWithToken <= 0 ? "destructive" : "default"} class="text-left">
			<TriangleAlertIcon class="size-4" />
			<AlertTitle class="text-sm">{emptyHelp.title}</AlertTitle>
			<AlertDescription class="text-xs leading-snug">{emptyHelp.description}</AlertDescription>
		</Alert>
	{:else}
		<div class="space-y-2" role="radiogroup" aria-label="Certificado de firma">
			{#each certs as c (c.id_hex)}
				{@const selected = certId === c.id_hex}
				{@const name = getHumanNameFromDn(c.subject_dn) || c.label || "(sin etiqueta)"}
				{@const dni = extractDniFromDn(c.subject_dn)}
				<button
					type="button"
					role="radio"
					aria-checked={selected}
					class={cn(
						"flex w-full items-start gap-3 rounded-lg border px-3 py-3 text-left transition-colors",
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
							"mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-full",
							selected ? "bg-primary/15 text-primary" : "bg-muted text-muted-foreground",
						)}
						aria-hidden="true"
					>
						<IdCardIcon class="size-4" />
					</span>
					<span class="min-w-0 flex-1 space-y-1">
						<span class="flex flex-wrap items-center gap-2">
							<span class="text-sm font-semibold leading-tight">{name}</span>
							<Badge variant="secondary" class="h-5 px-1.5 text-[10px] font-normal">
								{signingCertSourceLabel(c.source)}
							</Badge>
						</span>
						{#if dni}
							<span class="text-muted-foreground block text-xs tabular-nums">{dni}</span>
						{/if}
					</span>
					{#if selected}
						<CircleCheckIcon class="text-primary mt-1 size-5 shrink-0" aria-hidden="true" />
					{/if}
				</button>
			{/each}
		</div>
	{/if}
</div>
