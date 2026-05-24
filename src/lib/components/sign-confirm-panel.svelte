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
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";

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
	<Card.Header class="border-border/50 border-b pb-3">
		<Card.Title class="text-base font-semibold">Confirmar firma</Card.Title>
		<Card.Description class="text-xs leading-relaxed">
			{#if pinRequired}
				Comprueba el resumen e introduce el PIN de tu DNIe o tarjeta. El PIN no se guarda.
			{:else if selectedCert?.source === "win_my" && selectedCert.pin_ui === "os_may_prompt"}
				Comprueba el resumen. Al firmar, Windows o el dispositivo pueden pedir confirmación o PIN.
			{:else if selectedCert?.source === "win_my"}
				Comprueba el resumen. Este certificado usa el almacén de Windows: no hace falta PIN en NexoSign.
			{:else}
				Comprueba el resumen antes de firmar.
			{/if}
		</Card.Description>
	</Card.Header>

	<Card.Content class="px-4 pt-4 pb-4 sm:px-5">
		<div class="grid gap-6 sm:grid-cols-2 sm:items-start">
			<div
				class="border-border/60 bg-card w-full overflow-hidden rounded-lg border shadow-sm"
			>
				<p
					class="text-muted-foreground border-border/50 bg-muted/30 border-b px-3 py-2 text-[11px] font-medium tracking-wide uppercase"
				>
					Resumen
				</p>
				<dl class="divide-border/40 divide-y text-xs">
					<div class="flex items-start justify-between gap-4 px-3 py-2.5">
						<dt class="text-muted-foreground shrink-0">PDF</dt>
						<dd class="text-foreground font-semibold tabular-nums">{pathCount}</dd>
					</div>
					<div class="flex items-start justify-between gap-4 px-3 py-2.5">
						<dt class="text-muted-foreground shrink-0">Firma</dt>
						<dd class="min-w-0 flex-1 text-right leading-tight">
							{#if selectedCert}
								<span class="text-foreground font-medium">
									{getHumanNameFromDn(selectedCert.subject_dn) || "Titular"}
								</span>
								{#if extractDniFromDn(selectedCert.subject_dn)}
									<span class="text-muted-foreground mt-0.5 block text-[11px]">
										{extractDniFromDn(selectedCert.subject_dn)}
									</span>
								{/if}
							{:else}
								<span class="text-muted-foreground">—</span>
							{/if}
						</dd>
					</div>
					<div class="flex items-start justify-between gap-4 px-3 py-2.5">
						<dt class="text-muted-foreground shrink-0">Sello</dt>
						<dd class="text-foreground text-right font-medium">
							Col. {sigGridCol + 1}, fila {sigGridRow + 1}
						</dd>
					</div>
					<div class="flex items-start justify-between gap-4 px-3 py-2.5">
						<dt class="text-muted-foreground shrink-0">Salida</dt>
						<dd class="min-w-0 flex-1 text-right leading-tight">
							{#if outputDirForJob}
								<span class="font-medium" title={outputDirForJob}>
									«{pdfBasenameFromPath(outputDirForJob)}»
								</span>
							{:else}
								<code class="bg-muted rounded px-1.5 py-0.5 font-mono text-[11px]">*_firmado.pdf</code>
							{/if}
						</dd>
					</div>
				</dl>
			</div>

			<div class="space-y-3">
				{#if pinRequired}
					<div class="space-y-1.5">
						<Label for="pin-confirm" class="text-xs font-medium">PIN del DNIe o de la tarjeta</Label>
						<div class="relative">
							<Input
								id="pin-confirm"
								type={pinVisible ? "text" : "password"}
								autocomplete="off"
								bind:value={pin}
								placeholder="PIN"
								class={cn("h-10 w-full pr-10", pinError ? "border-destructive focus-visible:ring-destructive" : "")}
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
								class="text-muted-foreground absolute right-0.5 top-1/2 h-9 w-9 -translate-y-1/2"
								aria-label={pinVisible ? "Ocultar PIN" : "Mostrar PIN"}
								title={pinVisible ? "Ocultar PIN" : "Mostrar PIN"}
								onclick={() => {
									pinVisible = !pinVisible;
								}}
							>
								{#if pinVisible}
									<EyeOffIcon class="h-4 w-4" />
								{:else}
									<EyeIcon class="h-4 w-4" />
								{/if}
							</Button>
						</div>
						{#if pinError}
							<p class="text-xs font-medium text-destructive">{pinError}</p>
						{/if}
					</div>
				{:else if selectedCert?.source === "win_my"}
					<p class="text-muted-foreground text-xs leading-relaxed">
						{#if selectedCert.pin_ui === "os_may_prompt"}
							Windows o el dispositivo pueden pedir confirmación o PIN durante la firma.
						{:else}
							No hace falta introducir PIN aquí; la firma usa el almacén de credenciales de Windows.
						{/if}
					</p>
				{/if}

				<Button
					type="button"
					size="default"
					class="h-10 w-full gap-1"
					disabled={!canSubmit}
					onclick={() => void onSubmit()}
				>
					Firmar lote
					<ChevronRightIcon class="size-4 opacity-90" aria-hidden="true" />
				</Button>
			</div>
		</div>
	</Card.Content>
</Card.Root>
