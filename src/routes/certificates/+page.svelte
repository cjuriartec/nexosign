<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { Pkcs11ProbeCertificateListing, SigningCertSummary } from "$lib/tauri/pkcs11";
	import { isPkcs11NoTokenError } from "$lib/tauri/pkcs11-errors";
	import {
		hasPkcs11ChipCerts,
		probeTotalSlotsWithToken,
		signingCertListContextHint,
	} from "$lib/tauri/pkcs11-ux";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { invokeWithTimeout } from "$lib/tauri/invoke-timeout";
	import { getHumanNameFromDn, extractDniFromDn } from "$lib/signature-appearance";
	import SignatureAppearanceCard from "$lib/components/signature-appearance-card.svelte";
	import SigningCertPicker from "$lib/components/signing-cert-picker.svelte";
	import Loader2Icon from "@lucide/svelte/icons/loader-2";
	import IdCardIcon from "@lucide/svelte/icons/id-card";
	import KeyRoundIcon from "@lucide/svelte/icons/key-round";
	import StampIcon from "@lucide/svelte/icons/stamp";
	import { cn } from "$lib/utils.js";

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

	const selectedCert = $derived(certs.find((c) => c.id_hex === certId) ?? null);

	const selectedCertLabel = $derived.by(() => {
		if (!selectedCert) return null;
		const name = getHumanNameFromDn(selectedCert.subject_dn) || selectedCert.label || "Titular";
		const dni = extractDniFromDn(selectedCert.subject_dn);
		return dni ? `${name} · ${dni}` : name;
	});

	const statusLine = $derived.by(() => {
		if (busy) return "Actualizando certificados…";
		if (probeBusy) return "Diagnóstico del lector…";
		if (pinProbeBusy) return "Leyendo con PIN…";
		return null;
	});

	const listContextHint = $derived(
		isTauriRuntime() ? signingCertListContextHint(certs, slotsWithTokenCount) : null,
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

<div class="mx-auto flex w-full max-w-xl min-h-0 flex-1 flex-col gap-4 overflow-y-auto pb-6">
	<header class="flex flex-col items-center gap-2 pt-1 text-center">
		<div
			class="bg-primary/10 text-primary flex size-11 items-center justify-center rounded-full ring-1 ring-primary/20"
			aria-hidden="true"
		>
			<IdCardIcon class="size-5" />
		</div>
		<div class="space-y-0.5">
			<h1 class="text-xl font-semibold tracking-tight sm:text-2xl">Certificados</h1>
			<p class="text-muted-foreground text-sm leading-snug">
				DNIe o tarjeta · el PIN solo se pide al firmar
			</p>
		</div>
		{#if statusLine}
			<p class="text-muted-foreground flex items-center justify-center gap-1.5 text-xs">
				<Loader2Icon class="size-3.5 shrink-0 animate-spin" aria-hidden="true" />
				{statusLine}
			</p>
		{/if}
	</header>

	{#if !isTauriRuntime()}
		<section
			class="bg-card text-card-foreground flex flex-col items-center gap-3 rounded-xl border px-5 py-8 text-center shadow-sm"
		>
			<IdCardIcon class="text-muted-foreground size-10" aria-hidden="true" />
			<div class="space-y-1">
				<p class="text-sm font-semibold">App de escritorio</p>
				<p class="text-muted-foreground max-w-xs text-sm leading-snug">
					Instala NexoSign en Windows para leer el lector y configurar el sello.
				</p>
			</div>
		</section>
	{:else}
		<section
			class="bg-card text-card-foreground flex flex-col overflow-hidden rounded-xl border shadow-sm"
		>
			<div
				class="border-border/80 bg-muted/25 flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3"
			>
				<div class="min-w-0 text-left">
					<h2 class="text-sm font-semibold">Certificados de firma</h2>
					<p class="text-muted-foreground text-xs leading-snug">El que elijas se usará al firmar</p>
				</div>
				<div class="flex shrink-0 flex-wrap items-center gap-1.5">
					{#if slotsWithTokenCount > 0}
						<Badge variant="secondary" class="h-5 text-[10px] font-normal tabular-nums">
							{slotsWithTokenCount} lector{slotsWithTokenCount === 1 ? "" : "es"}
						</Badge>
					{/if}
					{#if certs.length > 0}
						<Badge variant="outline" class="h-5 text-[10px] tabular-nums">
							{certs.length} cert.
						</Badge>
					{/if}
				</div>
			</div>

			<div class="flex flex-col gap-3 p-4 sm:p-5">
				{#if showPinProbe}
					<div
						class="border-border/70 bg-muted/15 space-y-2.5 rounded-lg border border-dashed px-3 py-3"
					>
						<div class="flex items-start gap-2">
							<KeyRoundIcon class="text-muted-foreground mt-0.5 size-4 shrink-0" aria-hidden="true" />
							<div class="min-w-0 space-y-0.5">
								<p class="text-xs font-medium">Probar lectura con PIN</p>
								<p class="text-muted-foreground text-[11px] leading-snug">
									El lector responde, pero no aparece certificado de firma. Introduce el PIN del DNIe
									solo para comprobar el chip (no se guarda).
								</p>
							</div>
						</div>
						<div class="flex flex-wrap items-end gap-2">
							<div class="min-w-[8rem] flex-1 space-y-1">
								<Label for="cert-pin-probe" class="text-xs">PIN</Label>
								<Input
									id="cert-pin-probe"
									type="password"
									autocomplete="off"
									placeholder="••••"
									class="h-9"
									bind:value={probePin}
									disabled={pinProbeBusy || busy}
									onkeydown={(e) => {
										if (e.key === "Enter" && probePin.trim()) void tryListWithPin();
									}}
								/>
							</div>
							<Button
								variant="secondary"
								size="sm"
								class="h-9 shrink-0"
								disabled={pinProbeBusy || busy || !probePin.trim()}
								onclick={() => tryListWithPin()}
							>
								{pinProbeBusy ? "Leyendo…" : "Probar PIN"}
							</Button>
						</div>
					</div>
				{/if}

				<div class="flex max-h-[min(50vh,26rem)] min-h-0 flex-col">
					<SigningCertPicker
						{certs}
						bind:certId
						{busy}
						slotsWithToken={slotsWithTokenCount}
						contextHint={listContextHint}
						helpVariant="full"
						showDedupeNote={false}
						compact
						class="min-h-0 flex-1"
						onRefresh={() => loadCerts({ runChipProbe: true })}
						onResetReader={() => resetReaderAndReload()}
					/>
				</div>
			</div>
		</section>

		<section
			class={cn(
				"bg-card text-card-foreground flex flex-col overflow-hidden rounded-xl border shadow-sm transition-opacity",
				!selectedCert && certs.length > 0 && "opacity-95",
			)}
		>
			<div class="border-border/80 bg-muted/25 border-b px-4 py-3">
				<div class="flex items-start gap-2">
					<StampIcon class="text-muted-foreground mt-0.5 size-4 shrink-0" aria-hidden="true" />
					<div class="min-w-0 flex-1">
						<h2 class="text-sm font-semibold">Diseño del sello</h2>
						{#if selectedCertLabel}
							<p class="text-muted-foreground truncate text-xs" title={selectedCertLabel}>
								Vista previa con {selectedCertLabel}
							</p>
						{:else if certs.length > 0}
							<p class="text-muted-foreground text-xs">Selecciona un certificado arriba</p>
						{:else}
							<p class="text-muted-foreground text-xs">Vista de ejemplo hasta detectar un certificado</p>
						{/if}
					</div>
				</div>
			</div>

			<div class="p-4 sm:p-5">
				<SignatureAppearanceCard certs={previewCerts} compact hideHeader />
			</div>
		</section>
	{/if}
</div>
