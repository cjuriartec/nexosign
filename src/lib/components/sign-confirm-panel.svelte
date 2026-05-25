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

<Card.Root size="sm" class="mx-auto w-full max-w-md overflow-hidden border-0 bg-transparent shadow-none">
	<Card.Content class="py-3">
		<div class="grid grid-cols-2 items-start gap-4">
			<dl class="bg-muted/20 space-y-2.5 rounded-lg border px-3 py-2.5 text-xs">
				<div class="flex items-baseline justify-between gap-3">
					<dt class="text-muted-foreground shrink-0">PDF</dt>
					<dd class="font-semibold tabular-nums">{pathCount}</dd>
				</div>
				<div class="flex items-start justify-between gap-3">
					<dt class="text-muted-foreground shrink-0 pt-0.5">Firma</dt>
					<dd class="min-w-0 text-right">
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
				<div class="flex items-baseline justify-between gap-3">
					<dt class="text-muted-foreground shrink-0">Sello</dt>
					<dd class="font-medium tabular-nums">
						{sigGridCol + 1}·{sigGridRow + 1}
					</dd>
				</div>
				<div class="flex items-baseline justify-between gap-3">
					<dt class="text-muted-foreground shrink-0">Salida</dt>
					<dd class="min-w-0 truncate text-right font-medium" title={outputDirForJob ?? undefined}>
						{#if outputDirForJob}
							{pdfBasenameFromPath(outputDirForJob)}
						{:else}
							<code class="bg-muted rounded px-1 font-mono text-[10px] font-normal">*_firmado.pdf</code>
						{/if}
					</dd>
				</div>
			</dl>

			<div class="flex flex-col gap-3 sm:pt-0.5">
				{#if pinRequired}
					<div class="space-y-1.5">
						<Label for="pin-confirm" class="text-xs font-medium">PIN del certificado</Label>
						<div class="relative">
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
				{:else if selectedCert?.pin_ui === "os_may_prompt"}
					<p class="text-muted-foreground text-xs leading-snug">
						Windows puede pedir confirmación al firmar; no hace falta PIN en NexoSign.
					</p>
				{/if}

				<Button
					type="button"
					size="sm"
					class="h-9 w-full"
					disabled={!canSubmit}
					onclick={() => void onSubmit()}
				>
					{submitInFlight ? "Firmando…" : "Firmar lote"}
				</Button>
			</div>
		</div>
	</Card.Content>
</Card.Root>
