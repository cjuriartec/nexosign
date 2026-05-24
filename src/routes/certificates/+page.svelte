<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import * as Table from "$lib/components/ui/table/index.js";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { Pkcs11ProbeCertificateListing, SigningCertSummary } from "$lib/tauri/pkcs11";
	import { isPkcs11NoTokenError } from "$lib/tauri/pkcs11-errors";
	import {
		DEDUPED_WIN_MY_FOOTNOTE,
		emptySigningCertsHelp,
		hasPkcs11ChipCerts,
		onlyWinMySigningCerts,
		probeTotalSlotsWithToken,
		signingCertSourceLabel,
		winMyOnlyChipUnreadableMessage,
	} from "$lib/tauri/pkcs11-ux";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { invokeWithTimeout } from "$lib/tauri/invoke-timeout";
	import SignatureAppearanceCard from "$lib/components/signature-appearance-card.svelte";
	import { getHumanNameFromDn, extractDniFromDn, extractPurposeFromDn } from "$lib/signature-appearance";

	import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
	import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert/index.js";

	const LIST_TIMEOUT_MS = 45_000;
	const SLOT_TIMEOUT_MS = 12_000;
	const PROBE_TIMEOUT_MS = 50_000;

	let certs = $state<SigningCertSummary[]>([]);
	let slotsWithTokenCount = $state(0);
	let chipProbe = $state<Pkcs11ProbeCertificateListing | null>(null);
	let busy = $state(false);
	let probeBusy = $state(false);
	let probePin = $state("");
	let pinProbeBusy = $state(false);

	const winMyOnlyHint = $derived(
		isTauriRuntime() && onlyWinMySigningCerts(certs)
			? winMyOnlyChipUnreadableMessage(chipProbe, slotsWithTokenCount)
			: null,
	);
	const showPinProbe = $derived(
		isTauriRuntime() &&
			slotsWithTokenCount > 0 &&
			!hasPkcs11ChipCerts(certs),
	);

	async function refreshSlotCount() {
		try {
			slotsWithTokenCount = await invokeWithTimeout(
				pkcs11.pkcs11SlotCount(),
				SLOT_TIMEOUT_MS,
				"Conteo de lector",
			);
		} catch {
			slotsWithTokenCount = probeTotalSlotsWithToken(chipProbe);
		}
	}

	async function runChipProbeBackground() {
		if (!isTauriRuntime()) return;
		probeBusy = true;
		try {
			chipProbe = await invokeWithTimeout(
				pkcs11.pkcs11ProbeCertificateListing(),
				PROBE_TIMEOUT_MS,
				"Diagnóstico del chip",
			);
			const fromProbe = probeTotalSlotsWithToken(chipProbe);
			if (fromProbe > slotsWithTokenCount) {
				slotsWithTokenCount = fromProbe;
			}
		} catch (e) {
			toast.error(String(e));
		} finally {
			probeBusy = false;
		}
	}

	async function loadCerts(opts?: { runChipProbe?: boolean }) {
		if (!isTauriRuntime()) return;
		busy = true;
		try {
			certs = await invokeWithTimeout(
				pkcs11.listSigningCertificates(),
				LIST_TIMEOUT_MS,
				"Carga de certificados",
			);
		} catch (e) {
			certs = [];
			if (isPkcs11NoTokenError(e)) {
				return;
			}
			toast.error(String(e));
		} finally {
			busy = false;
		}
		await refreshSlotCount();
		if (opts?.runChipProbe) {
			void runChipProbeBackground();
		}
	}

	async function tryListWithPin() {
		if (!isTauriRuntime() || !probePin.trim()) {
			toast.error("Introduce el PIN del DNIe.");
			return;
		}
		pinProbeBusy = true;
		try {
			certs = await invokeWithTimeout(
				pkcs11.pkcs11ListSigningWithPin(probePin.trim()),
				LIST_TIMEOUT_MS,
				"Lectura con PIN",
			);
			await refreshSlotCount();
			if (hasPkcs11ChipCerts(certs)) {
				toast.success("Certificado del lector detectado");
			} else {
				toast.message("PIN aceptado, pero no hay certificado de firma en el chip");
			}
		} catch (e) {
			toast.error(String(e));
		} finally {
			await pkcs11.pkcs11ResetConnection().catch(() => {});
			probePin = "";
			pinProbeBusy = false;
			void runChipProbeBackground();
		}
	}

	async function resetReaderAndReload() {
		if (!isTauriRuntime()) return;
		busy = true;
		try {
			await invokeWithTimeout(pkcs11.pkcs11ResetConnection(), SLOT_TIMEOUT_MS, "Reinicializar lector");
			await loadCerts({ runChipProbe: true });
			toast.success("Lector reinicializado");
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	onMount(() => {
		if (!isTauriRuntime()) return;
		void loadCerts({ runChipProbe: true });
	});
</script>

<svelte:head>
	<title>Certificados — NexoSign</title>
</svelte:head>

<div class="mx-auto max-w-6xl space-y-8">
	<div>
		<h1 class="text-3xl font-semibold tracking-tight">Certificados para firma</h1>
		<p class="text-muted-foreground mt-1 max-w-2xl text-sm">
			Certificados disponibles en tu DNIe o tarjeta. El PIN solo lo pedimos al firmar.
		</p>
	</div>

	{#if !isTauriRuntime()}
		<Card.Root>
			<Card.Header>
				<Card.Title>Solo en la app de escritorio</Card.Title>
				<Card.Description>
					Para leer los certificados del lector necesitas la app de NexoSign instalada.
				</Card.Description>
			</Card.Header>
		</Card.Root>
	{:else}
		<Card.Root>
			<Card.Header class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
				<div>
					<Card.Title class="text-base">Certificados</Card.Title>
					<Card.Description>
						Lista actual desde tu DNIe o tarjeta. Pulsa «Recargar» para actualizar tras conectar el lector.
						<span class="text-muted-foreground mt-1 block text-xs leading-snug">{DEDUPED_WIN_MY_FOOTNOTE}</span>
						{#if probeBusy}
							<span class="text-muted-foreground block text-xs">Comprobando chip en segundo plano…</span>
						{/if}
					</Card.Description>
				</div>
				<div class="flex shrink-0 flex-wrap gap-2 self-start">
					<Button variant="outline" size="sm" disabled={busy} onclick={() => loadCerts({ runChipProbe: true })}>
						{busy ? "Cargando…" : "Recargar"}
					</Button>
					<Button variant="outline" size="sm" disabled={busy} onclick={() => resetReaderAndReload()}>
						Reinicializar lector
					</Button>
				</div>
			</Card.Header>
			<Card.Content class="space-y-4">
				{#if winMyOnlyHint}
					<Alert class="text-left">
						<TriangleAlertIcon class="size-4" />
						<AlertTitle class="text-sm">Certificado en Windows, no en el lector</AlertTitle>
						<AlertDescription class="text-xs leading-snug">{winMyOnlyHint}</AlertDescription>
					</Alert>
				{/if}
				{#if showPinProbe}
					<div class="flex flex-col gap-2 rounded-md border p-3 sm:flex-row sm:items-end">
						<div class="min-w-0 flex-1 space-y-1">
							<Label for="probe-pin" class="text-xs">Probar lectura del chip con PIN</Label>
							<Input
								id="probe-pin"
								type="password"
								autocomplete="off"
								placeholder="PIN del DNIe"
								bind:value={probePin}
								disabled={pinProbeBusy || busy}
							/>
						</div>
						<Button
							variant="secondary"
							size="sm"
							class="shrink-0"
							disabled={pinProbeBusy || busy || !probePin.trim()}
							onclick={() => tryListWithPin()}
						>
							{pinProbeBusy ? "Leyendo…" : "Probar con PIN"}
						</Button>
					</div>
				{/if}
				{#if certs.length === 0}
					{@const help = emptySigningCertsHelp(slotsWithTokenCount)}
					<Alert variant={slotsWithTokenCount <= 0 ? "destructive" : "default"} class="text-left">
						<TriangleAlertIcon class="size-4" />
						<AlertTitle class="text-sm">{help.title}</AlertTitle>
						<AlertDescription class="text-xs leading-snug">{help.description}</AlertDescription>
					</Alert>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head>Titular</Table.Head>
								<Table.Head>Documento</Table.Head>
								<Table.Head>Origen</Table.Head>
								<Table.Head class="text-right">Uso</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each certs as c}
								<Table.Row>
									<Table.Cell class="font-medium">{getHumanNameFromDn(c.subject_dn) || c.label || "—"}</Table.Cell>
									<Table.Cell class="text-muted-foreground text-sm">{extractDniFromDn(c.subject_dn) || "—"}</Table.Cell>
									<Table.Cell class="text-muted-foreground text-xs">
										{signingCertSourceLabel(c.source)}
										{#if c.source === "pkcs11"}
											<span class="text-foreground/70 block text-[10px]">Recomendado con tarjeta insertada</span>
										{/if}
									</Table.Cell>
									<Table.Cell class="text-right">
										<span class="inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 border-transparent bg-secondary text-secondary-foreground hover:bg-secondary/80">
											{extractPurposeFromDn(c.subject_dn)}
										</span>
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				{/if}
			</Card.Content>
		</Card.Root>

	{/if}

	<Card.Root class="border-border/50 shadow-none">
		<Card.Content class="min-w-0 p-3 md:p-4">
			<SignatureAppearanceCard certs={isTauriRuntime() ? certs : []} />
		</Card.Content>
	</Card.Root>
</div>
