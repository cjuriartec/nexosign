<script lang="ts">
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import { pinRequiredInApp } from "$lib/tauri/pkcs11";
	import { getHumanNameFromDn, extractDniFromDn } from "$lib/signature-appearance";
	import { pdfBasenameFromPath } from "$lib/sign/path-util";
	import { cn } from "$lib/utils.js";
	import EyeIcon from "@lucide/svelte/icons/eye";
	import EyeOffIcon from "@lucide/svelte/icons/eye-off";

	interface Props {
		pathCount: number;
		selectedCert: SigningCertSummary | null;
		sigGridCol: number;
		sigGridRow: number;
		outputDirForJob: string | null;
		pin?: string;
		pinError?: string | null;
		pinVisible?: boolean;
		busy?: boolean;
		submitInFlight?: boolean;
		onSubmit: () => void | Promise<void>;
	}

	let {
		pathCount,
		selectedCert,
		sigGridCol,
		sigGridRow,
		outputDirForJob,
		pin = $bindable(""),
		pinError = $bindable(null),
		pinVisible = $bindable(false),
		busy = false,
		submitInFlight = false,
		onSubmit,
	}: Props = $props();

	const pinRequired = $derived(pinRequiredInApp(selectedCert));

	const canSubmit = $derived(
		!busy &&
			!submitInFlight &&
			pathCount > 0 &&
			!!selectedCert &&
			(!pinRequired || pin.trim().length > 0),
	);
</script>

<Card.Root size="sm" class="w-full overflow-hidden">
	<Card.Header class="pb-2">
		<Card.Title class="text-sm font-medium">Confirmar</Card.Title>
	</Card.Header>

	<Card.Content class="space-y-3 pt-0 pb-3">
		<dl class="divide-border/50 grid gap-0 divide-y rounded-lg border text-xs sm:grid-cols-2 sm:divide-y-0 sm:gap-x-4">
			<div class="flex items-center justify-between gap-3 px-3 py-2 sm:flex-col sm:items-start sm:gap-0.5 sm:py-2.5">
				<dt class="text-muted-foreground">PDF</dt>
				<dd class="font-semibold tabular-nums">{pathCount}</dd>
			</div>
			<div class="flex items-center justify-between gap-3 border-t px-3 py-2 sm:border-t-0 sm:py-2.5">
				<dt class="text-muted-foreground shrink-0">Firma</dt>
				<dd class="min-w-0 text-right sm:text-left">
					{#if selectedCert}
						<span class="font-medium">
							{getHumanNameFromDn(selectedCert.subject_dn) || "Titular"}
						</span>
						{#if extractDniFromDn(selectedCert.subject_dn)}
							<span class="text-muted-foreground block text-[11px]">
								{extractDniFromDn(selectedCert.subject_dn)}
							</span>
						{/if}
					{:else}
						<span class="text-muted-foreground">—</span>
					{/if}
				</dd>
			</div>
			<div class="flex items-center justify-between gap-3 border-t px-3 py-2 sm:border-t-0 sm:py-2.5">
				<dt class="text-muted-foreground">Sello</dt>
				<dd class="font-medium tabular-nums">
					{sigGridCol + 1}·{sigGridRow + 1}
				</dd>
			</div>
			<div class="flex items-center justify-between gap-3 border-t px-3 py-2 sm:border-t-0 sm:py-2.5">
				<dt class="text-muted-foreground shrink-0">Salida</dt>
				<dd class="min-w-0 truncate text-right sm:text-left" title={outputDirForJob ?? undefined}>
					{#if outputDirForJob}
						{pdfBasenameFromPath(outputDirForJob)}
					{:else}
						<code class="bg-muted rounded px-1 font-mono text-[10px]">*_firmado.pdf</code>
					{/if}
				</dd>
			</div>
		</dl>

		{#if pinRequired}
			<div class="space-y-1.5">
				<Label for="pin-confirm" class="text-xs font-medium">PIN</Label>
				<div class="relative max-w-xs">
					<Input
						id="pin-confirm"
						type={pinVisible ? "text" : "password"}
						autocomplete="off"
						bind:value={pin}
						placeholder="••••"
						class={cn("h-9 w-full pr-10", pinError ? "border-destructive" : "")}
						oninput={() => {
							pinError = null;
						}}
						onkeydown={(e) => {
							if (e.key === "Enter" && canSubmit) {
								e.preventDefault();
								void onSubmit();
							}
						}}
					/>
					<Button
						type="button"
						variant="ghost"
						size="icon"
						class="text-muted-foreground absolute right-0.5 top-1/2 size-8 -translate-y-1/2"
						aria-label={pinVisible ? "Ocultar PIN" : "Mostrar PIN"}
						onclick={() => {
							pinVisible = !pinVisible;
						}}
					>
						{#if pinVisible}
							<EyeOffIcon class="size-4" />
						{:else}
							<EyeIcon class="size-4" />
						{/if}
					</Button>
				</div>
				{#if pinError}
					<p class="text-destructive text-xs font-medium">{pinError}</p>
				{/if}
			</div>
		{/if}

		<Button
			type="button"
			size="sm"
			class="h-9 w-full max-w-xs"
			disabled={!canSubmit}
			onclick={() => void onSubmit()}
		>
			Firmar
		</Button>
	</Card.Content>
</Card.Root>
