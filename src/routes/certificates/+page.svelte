<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { Pkcs11ProbeCertificateListing, SigningCertSummary } from "$lib/tauri/pkcs11";
	import { isPkcs11NoTokenError } from "$lib/tauri/pkcs11-errors";
	import {
		hasPkcs11ChipCerts,
		onlyWinMySigningCerts,
		probeTotalSlotsWithToken,
		winMyOnlyHintBrief,
	} from "$lib/tauri/pkcs11-ux";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { invokeWithTimeout } from "$lib/tauri/invoke-timeout";
	import SignatureAppearanceCard from "$lib/components/signature-appearance-card.svelte";
	import SigningCertPicker from "$lib/components/signing-cert-picker.svelte";
	import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert/index.js";
	import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
	import Loader2Icon from "@lucide/svelte/icons/loader-2";

	const LIST_TIMEOUT_MS = 45_000;
	const SLOT_TIMEOUT_MS = 12_000;
	const PROBE_TIMEOUT_MS = 50_000;

	let certs = $state<SigningCertSummary[]>([]);
	let certId = $state("");
	let slotsWithTokenCount = $state(0);
	let chipProbe = $state<Pkcs11ProbeCertificateListing | null>(null);
	let busy = $state(false);
	let probeBusy = $state(false);
	let probePin = $state("");
	let pinProbeBusy = $state(false);

	const previewCerts = $derived(
		certId ? certs.filter((c) => c.id_hex === certId) : certs.length ? [certs[0]] : [],
	);

	const winMyOnlyHint = $derived(
		isTauriRuntime() && onlyWinMySigningCerts(certs)
			? winMyOnlyHintBrief(chipProbe, slotsWithTokenCount)
			: null,
	);
	const showPinProbe = $derived(
		isTauriRuntime() && slotsWithTokenCount > 0 && !hasPkcs11ChipCerts(certs),
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
			if (certs.length && !certId) {
				certId = certs[0]?.id_hex ?? "";
			}
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
			if (certs.length && !certId) {
				certId = certs[0]?.id_hex ?? "";
			}
			await refreshSlotCount();
			if (hasPkcs11ChipCerts(certs)) {
				toast.success("Certificado del lector detectado");
			} else {
				toast.message("PIN correcto, sin certificado de firma en chip");
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
			await invokeWithTimeout(pkcs11.pkcs11ResetConnection(), SLOT_TIMEOUT_MS, "Reconectar lector");
			await loadCerts({ runChipProbe: true });
			toast.success("Lector reconectado");
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

<div class="mx-auto max-w-6xl space-y-6">
	<div class="flex flex-wrap items-end justify-between gap-3">
		<div>
			<h1 class="text-2xl font-semibold tracking-tight">Certificados</h1>
			<p class="text-muted-foreground mt-0.5 text-sm">DNIe o tarjeta · PIN solo al firmar</p>
		</div>
		{#if isTauriRuntime() && probeBusy}
			<Loader2Icon class="text-muted-foreground size-4 animate-spin" aria-hidden="true" />
		{/if}
	</div>

	{#if !isTauriRuntime()}
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">App de escritorio</Card.Title>
				<Card.Description class="text-sm">Instala NexoSign para leer el lector.</Card.Description>
			</Card.Header>
		</Card.Root>
	{:else}
		<Card.Root>
			<Card.Content class="space-y-3 pt-4">
				{#if winMyOnlyHint}
					<Alert class="py-2">
						<TriangleAlertIcon class="size-4" />
						<AlertTitle class="text-sm">Solo en Windows</AlertTitle>
						<AlertDescription class="text-xs">{winMyOnlyHint}</AlertDescription>
					</Alert>
				{/if}

				{#if showPinProbe}
					<div class="flex gap-2">
						<Input
							type="password"
							autocomplete="off"
							placeholder="PIN"
							class="h-9 max-w-[8rem]"
							bind:value={probePin}
							disabled={pinProbeBusy || busy}
							onkeydown={(e) => {
								if (e.key === "Enter" && probePin.trim()) void tryListWithPin();
							}}
						/>
						<Button
							variant="secondary"
							size="sm"
							class="h-9"
							disabled={pinProbeBusy || busy || !probePin.trim()}
							onclick={() => tryListWithPin()}
						>
							{pinProbeBusy ? "…" : "Probar PIN"}
						</Button>
					</div>
				{/if}

				<SigningCertPicker
					{certs}
					bind:certId
					{busy}
					slotsWithToken={slotsWithTokenCount}
					helpVariant="brief"
					showDedupeNote={false}
					onRefresh={() => loadCerts({ runChipProbe: true })}
					onResetReader={() => resetReaderAndReload()}
				/>
			</Card.Content>
		</Card.Root>
	{/if}

	<Card.Root class="border-border/50 shadow-none">
		<Card.Content class="min-w-0 p-3 md:p-4">
			<SignatureAppearanceCard certs={previewCerts} compact />
		</Card.Content>
	</Card.Root>
</div>
