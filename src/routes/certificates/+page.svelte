<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "$lib/ui/notify";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import * as Tabs from "$lib/components/ui/tabs/index.js";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { Pkcs11ProbeCertificateListing, SigningCertSummary } from "$lib/tauri/pkcs11";
	import { isPkcs11NoTokenError } from "$lib/tauri/pkcs11-errors";
	import {
		hasPkcs11ChipCerts,
		probeTotalSlotsWithToken,
		signingCertListContextHint,
		signingCertSourceLabel,
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

	const LIST_TIMEOUT_MS = 45_000;
	const SLOT_TIMEOUT_MS = 12_000;
	const PROBE_TIMEOUT_MS = 50_000;

	let certs = $state<SigningCertSummary[]>([]);
	let certId = $state("");
	let activeTab = $state("cert");
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

	const selectedCertShort = $derived.by(() => {
		if (!selectedCert) return null;
		const name = getHumanNameFromDn(selectedCert.subject_dn) || selectedCert.label || "Titular";
		const dni = extractDniFromDn(selectedCert.subject_dn);
		return { name, dni, source: signingCertSourceLabel(selectedCert.source) };
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

	type LoadCertsOpts = { runChipProbe?: boolean };

	async function loadCerts(opts: LoadCertsOpts = {}) {
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

<div class="flex min-h-0 min-w-0 flex-1 flex-col">
	<div class="mb-2 flex shrink-0 items-start justify-between gap-2">
		<div class="min-w-0">
			<h1 class="text-base font-semibold tracking-tight">Certificados</h1>
			<p class="text-muted-foreground text-[11px] leading-snug">DNIe o tarjeta · PIN solo al firmar</p>
		</div>
		{#if statusLine}
			<p
				class="text-muted-foreground flex max-w-[45%] shrink-0 items-center justify-end gap-1 text-right text-[10px] leading-tight"
			>
				<Loader2Icon class="size-3 shrink-0 animate-spin" aria-hidden="true" />
				<span class="line-clamp-2">{statusLine}</span>
			</p>
		{/if}
	</div>

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
			class="bg-card text-card-foreground flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-xl border shadow-sm"
		>
			<Tabs.Root bind:value={activeTab} class="flex min-h-0 flex-1 flex-col gap-0">
				<div class="border-border/80 shrink-0 border-b px-3 pt-2.5 pb-2.5">
					<Tabs.List variant="line" class="h-9 w-full">
						<Tabs.Trigger value="cert" class="gap-1.5 text-xs sm:text-sm">
							<IdCardIcon class="size-3.5 shrink-0" aria-hidden="true" />
							Certificado
						</Tabs.Trigger>
						<Tabs.Trigger value="stamp" class="gap-1.5 text-xs sm:text-sm">
							<StampIcon class="size-3.5 shrink-0" aria-hidden="true" />
							Sello
						</Tabs.Trigger>
					</Tabs.List>
				</div>

				<Tabs.Content
					value="cert"
					class="scrollbar-subtle min-h-0 flex-1 overflow-x-hidden overflow-y-auto outline-none"
				>
					<div class="flex min-w-0 flex-col gap-2.5 p-3">
						{#if showPinProbe}
							<div
								class="border-border/70 bg-muted/15 space-y-2 rounded-lg border border-dashed px-2.5 py-2.5"
							>
								<div class="flex items-start gap-2">
									<KeyRoundIcon
										class="text-muted-foreground mt-0.5 size-3.5 shrink-0"
										aria-hidden="true"
									/>
									<p class="text-muted-foreground text-[11px] leading-snug">
										Lector detectado sin certificado visible. Prueba con PIN (no se guarda).
									</p>
								</div>
								<div class="flex items-end gap-2">
									<div class="min-w-0 flex-1 space-y-1">
										<Label for="cert-pin-probe" class="text-xs">PIN</Label>
										<Input
											id="cert-pin-probe"
											type="password"
											autocomplete="off"
											placeholder="••••"
											class="h-8"
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
										class="h-8 shrink-0 px-2.5 text-xs"
										disabled={pinProbeBusy || busy || !probePin.trim()}
										onclick={() => tryListWithPin()}
									>
										{pinProbeBusy ? "…" : "Probar"}
									</Button>
								</div>
							</div>
						{/if}

						<SigningCertPicker
							{certs}
							bind:certId
							{busy}
							slotsWithToken={slotsWithTokenCount}
							contextHint={listContextHint}
							helpVariant="brief"
							showDedupeNote={false}
							compact
							emptyPresentation="panel"
							onRefresh={() => loadCerts({ runChipProbe: true })}
							onResetReader={() => resetReaderAndReload()}
						/>
					</div>
				</Tabs.Content>

				<Tabs.Content
					value="stamp"
					class="scrollbar-subtle min-h-0 flex-1 overflow-x-hidden overflow-y-auto outline-none"
				>
					<div class="min-w-0 p-3">
						{#if !selectedCert}
							<div
								class="border-border/60 bg-muted/10 flex flex-col items-center gap-2.5 rounded-xl border border-dashed px-4 py-8 text-center"
							>
								<span
									class="bg-muted text-muted-foreground flex size-10 items-center justify-center rounded-full"
									aria-hidden="true"
								>
									<StampIcon class="size-4" />
								</span>
								<div class="max-w-[16rem] space-y-1">
									<p class="text-sm font-medium leading-snug">
										{#if certs.length === 0}
											Sin certificado de firma
										{:else}
											Elige un certificado
										{/if}
									</p>
									<p class="text-muted-foreground text-xs leading-snug">
										{#if certs.length === 0}
											Conecta el DNIe en la pestaña Certificado para personalizar el sello con tus datos.
										{:else}
											Selecciónalo en la pestaña Certificado para ver la vista previa con tu nombre y DNI.
										{/if}
									</p>
								</div>
								<Button
									type="button"
									variant="outline"
									size="sm"
									class="h-8 text-xs"
									onclick={() => {
										activeTab = "cert";
									}}
								>
									Ir a Certificado
								</Button>
							</div>
						{:else}
							{#if selectedCertShort}
								<p class="text-muted-foreground mb-2.5 text-[11px] leading-snug">
									Vista previa con
									<span class="text-foreground font-medium">{selectedCertShort.name}</span>
									{#if selectedCertShort.dni}
										· <span class="tabular-nums">{selectedCertShort.dni}</span>
									{/if}
								</p>
							{/if}
							<SignatureAppearanceCard certs={previewCerts} compact previewFirst hideHeader />
						{/if}
					</div>
				</Tabs.Content>
			</Tabs.Root>
		</section>
	{/if}
</div>
