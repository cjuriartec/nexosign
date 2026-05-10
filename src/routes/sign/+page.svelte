<script lang="ts">
	import { onMount } from "svelte";
	import { open, ask } from "@tauri-apps/plugin-dialog";
	import { basename, dirname, join } from "@tauri-apps/api/path";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import * as Table from "$lib/components/ui/table/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { Progress } from "$lib/components/ui/progress/index.js";
	import * as ScrollArea from "$lib/components/ui/scroll-area/index.js";
	import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert/index.js";
	import { page } from "$app/state";
	import { postBatchSign, type BatchSignBody } from "$lib/api/local-api";
	import { subscribeProgress, type ProgressPayload } from "$lib/events/progress";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import { enumeratePdfsUnderFolder } from "$lib/tauri/batch";
	import { getBatchSignIntent } from "$lib/tauri/batch-sign-intent";
	import { isPkcs11NoTokenError } from "$lib/tauri/pkcs11-errors";
	import { cancelBatchJob, getLocalApiBaseUrl } from "$lib/tauri/settings";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { getHumanNameFromDn, extractDniFromDn } from "$lib/signature-appearance";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import FileStackIcon from "@lucide/svelte/icons/files";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import CheckIcon from "@lucide/svelte/icons/check";
	import { cn } from "$lib/utils.js";

	const SIGN_STEPS = [
		{ step: 1, title: "Archivos", hint: "PDF sueltos o carpeta entera" },
		{ step: 2, title: "Certificado", hint: "Tu identidad para firmar" },
		{ step: 3, title: "PIN", hint: "Del DNIe o tarjeta (no se guarda)" },
		{ step: 4, title: "Ubicación", hint: "Casilla en la 1.ª página (pie discreto)" },
		{ step: 5, title: "Confirmar", hint: "Revisa y pulsa Firmar" },
	] as const;

	/** Rejilla 7×5 en primera página: col 0 izquierda, fila 0 arriba (como se lee el PDF). */
	const SIG_GRID_COLS = 7;

	let paths = $state<string[]>([]);
	/** Origen del lote actual: archivos sueltos vs carpeta (salida agrupada). */
	let sourceMode = $state<"files" | "folder" | null>(null);
	let folderPath = $state<string | null>(null);
	/** Directorio absoluto `{padre}/{nombre}_firmados` cuando sourceMode === folder */
	let outputDirForJob = $state<string | null>(null);

	let certs = $state<SigningCertSummary[]>([]);
	let certId = $state("");
	let pin = $state("");
	let pinError = $state<string | null>(null);
	let apiBase = $state("");
	let busy = $state(false);

	/** 1 archivos · 2 cert · 3 PIN · 4 ubicación · 5 confirmar */
	let wizardStep = $state(1);

	let sigGridCol = $state(3);
	let sigGridRow = $state(4);

	/** Si viene de `POST /api/v1/batch/sign/intent`, se envía al confirmar para cerrar la intención. */
	let intentRequestId = $state<string | null>(null);
	/** Evita ejecutar dos veces la misma `?intent=` si `replaceState` no actualiza al instante el store de rutas. */
	let handledIntentQuery = $state<string | null>(null);

	let activeJobId = $state<string | null>(null);
	const activeJobRef: { current: string | null } = { current: null };
	let progressPct = $state(0);
	let logLines = $state<string[]>([]);

	const showProgressPanel = $derived(
		activeJobId !== null || logLines.length > 0 || progressPct > 0,
	);

	const wizardBarPct = $derived(Math.round((wizardStep / 5) * 100));

	function pushLog(line: string) {
		logLines = [...logLines, line].slice(-120);
	}

	async function refreshCerts(): Promise<number> {
		if (!isTauriRuntime()) return 0;
		try {
			certs = await pkcs11.listSigningCertificates();
			if (certs.length && !certId) {
				certId = certs[0]?.id_hex ?? "";
			}
			return certs.length;
		} catch (e) {
			certs = [];
			if (isPkcs11NoTokenError(e)) {
				return 0;
			}
			toast.error(String(e));
			return 0;
		}
	}

	/** Libera el driver del token en memoria y vuelve a enumerar certificados. */
	async function resetPkcs11ConnectionAndRefresh() {
		if (!isTauriRuntime()) return;
		busy = true;
		try {
			await pkcs11.pkcs11ResetConnection();
			const n = await refreshCerts();
			toast.success(
				n > 0
					? "Listo: ya puedes elegir tu certificado."
					: "Conexión reiniciada. Comprueba que la tarjeta esté bien puesta y pulsa «Actualizar lista».",
			);
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	/** Confirma en lenguaje claro antes de cortar la sesión con el lector (evita clics accidentales). */
	async function confirmResetReader() {
		if (!isTauriRuntime()) return;
		const ok = await ask(
			"NexoSign cerrará la conexión con tu lector o tarjeta y volverá a buscar certificados. Tus PDF no se modifican.\n\n¿Seguir?",
			{
				title: "Volver a detectar la tarjeta",
				kind: "info",
			},
		);
		if (!ok) return;
		await resetPkcs11ConnectionAndRefresh();
	}

	async function computeFirmadosDir(folderAbs: string): Promise<string> {
		const parent = await dirname(folderAbs);
		const base = await basename(folderAbs);
		return join(parent, `${base}_firmados`);
	}

	async function pickPdfs() {
		if (!isTauriRuntime()) {
			toast.error("La selección de archivos requiere la app de escritorio.");
			return;
		}
		const sel = await open({
			multiple: true,
			filters: [{ name: "PDF", extensions: ["pdf"] }],
		});
		if (sel === null) return;
		const list = Array.isArray(sel) ? sel : [sel];
		sourceMode = "files";
		folderPath = null;
		outputDirForJob = null;
		const unique = [...new Set(list)];
		paths = unique;
		intentRequestId = null;
	}

	async function pickFolder() {
		if (!isTauriRuntime()) {
			toast.error("Requiere la app de escritorio.");
			return;
		}
		const sel = await open({ directory: true, multiple: false });
		if (sel === null || Array.isArray(sel)) return;
		busy = true;
		try {
			const pdfs = await enumeratePdfsUnderFolder(sel);
			paths = pdfs;
			sourceMode = "folder";
			folderPath = sel;
			outputDirForJob = await computeFirmadosDir(sel);
			intentRequestId = null;
			if (pdfs.length === 0) {
				toast.message("No hay PDFs en esa carpeta.");
			}
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	async function applyPendingIntent(intentParam: string) {
		const payload = await getBatchSignIntent(intentParam);
		if (!payload) {
			toast.error("La solicitud no existe o caducó (30 min). Abre el enlace desde la integración de nuevo.");
			return;
		}
		paths = [...payload.inputs];
		sourceMode = "files";
		folderPath = null;
		outputDirForJob = payload.outputDir ?? null;
		intentRequestId = intentParam;
		await refreshCerts();
		wizardStep = 1;
		toast.message("Lote recibido desde la integración: revisa los pasos y confirma aquí.");
		if (typeof window !== "undefined") {
			const u = new URL(window.location.href);
			u.searchParams.delete("intent");
			history.replaceState({}, "", `${u.pathname}${u.search}${u.hash}`);
		}
	}

	function clearPaths() {
		paths = [];
		sourceMode = null;
		folderPath = null;
		outputDirForJob = null;
		intentRequestId = null;
	}

	function removeAt(i: number) {
		paths = paths.filter((_, idx) => idx !== i);
		if (paths.length === 0) clearPaths();
	}

	async function step1Continue() {
		if (paths.length === 0) return;
		await refreshCerts();
		wizardStep = 2;
	}

	async function step2Continue() {
		if (!certId.trim()) {
			toast.error("Selecciona un certificado.");
			return;
		}
		wizardStep = 3;
	}

	async function pinStepContinue() {
		if (!pin.trim()) return;
		busy = true;
		pinError = null;
		try {
			if (isTauriRuntime()) {
				await pkcs11.pkcs11Login(pin.trim());
			}
			wizardStep = 4;
		} catch (e) {
			const msg = String(e);
			if (msg.includes("PIN is incorrect") || msg.includes("CKR_PIN_INCORRECT")) {
				pinError = "El PIN ingresado es incorrecto. Inténtalo de nuevo.";
			} else if (msg.includes("LOCKED") || msg.includes("CKR_PIN_LOCKED")) {
				pinError = "El DNI/Token ha sido bloqueado por múltiples intentos fallidos.";
			} else {
				pinError = `Error al verificar: ${msg}`;
			}
		} finally {
			busy = false;
		}
	}

	function placementStepContinue() {
		wizardStep = 5;
	}

	function goBack() {
		if (wizardStep <= 1) return;
		wizardStep -= 1;
	}

	function jumpToStep(step: number) {
		if (step < 1 || step >= wizardStep) return;
		wizardStep = step;
	}

	async function submitBatch() {
		if (!certId.trim()) {
			toast.error("Selecciona un certificado.");
			return;
		}
		if (paths.length === 0) {
			toast.error("No hay PDFs.");
			return;
		}
		if (!pin.trim()) {
			toast.error("Indica el PIN del token en el paso 3.");
			wizardStep = 3;
			return;
		}

		busy = true;
		logLines = [];
		progressPct = 0;
		activeJobId = null;
		activeJobRef.current = null;
		try {
			const body: BatchSignBody = {
				cert_id_hex: certId.trim(),
				inputs: paths,
				pin: pin.trim(),
				signature_grid: { col: sigGridCol, row: sigGridRow },
			};
			if (outputDirForJob) {
				body.output_dir = outputDirForJob;
			}
			if (intentRequestId) {
				body.intent_request_id = intentRequestId;
			}
			const res = await postBatchSign(body, apiBase);
			activeJobId = res.job_id;
			activeJobRef.current = res.job_id;
			intentRequestId = null;
			toast.success("Firma en curso");
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	async function cancelJob() {
		const id = activeJobId;
		if (!id) {
			toast.message("No hay una firma reciente en cola.");
			return;
		}
		if (!isTauriRuntime()) return;
		try {
			const ok = await cancelBatchJob(id);
			toast.message(ok ? "Cancelación enviada" : "Trabajo no encontrado");
		} catch (e) {
			toast.error(String(e));
		}
	}

	$effect(() => {
		const q = page.url.searchParams.get("intent");
		if (!isTauriRuntime() || !q) return;
		if (handledIntentQuery === q) return;
		handledIntentQuery = q;
		void applyPendingIntent(q);
	});

	onMount(() => {
		let unlisten: (() => void) | undefined;

		void (async () => {
			if (isTauriRuntime()) {
				apiBase = await getLocalApiBaseUrl();
				await refreshCerts();
			} else {
				apiBase = "http://127.0.0.1:14500";
			}

			try {
				unlisten = await subscribeProgress((p: ProgressPayload) => {
					if (!activeJobRef.current || p.job_id !== activeJobRef.current) return;
					const total = Math.max(1, p.total);
					progressPct = Math.min(100, Math.round((100 * p.actual) / total));
					const tail = p.nombre_archivo || p.path || "";
					const err = p.error ? ` · ${p.error}` : "";
					pushLog(`${p.actual}/${p.total} · ${tail}${err}`);
				});
			} catch {
				/* sin Tauri event */
			}
		})();

		return () => unlisten?.();
	});

	$effect(() => {
		activeJobRef.current = activeJobId;
	});
</script>

<svelte:head>
	<title>Firmar — NexoSign</title>
</svelte:head>

<div class="space-y-8">
	<div class="flex flex-wrap items-start justify-between gap-4">
		<div>
			<h1 class="text-3xl font-semibold tracking-tight">Firmar</h1>
			<p class="text-muted-foreground mt-1 text-sm">
				Paso <span class="text-foreground font-medium">{wizardStep}</span> de 5 · sigue el orden o vuelve atrás con el botón o tocando un paso ya hecho.
			</p>
		</div>
		{#if wizardStep > 1}
			<Button variant="outline" size="sm" onclick={() => goBack()} class="gap-1">
				<ChevronLeftIcon class="size-4" />
				Atrás
			</Button>
		{/if}
	</div>

	<nav class="space-y-3" aria-label="Pasos del asistente de firma">
		<div class="grid grid-cols-5 gap-1.5 sm:gap-3">
			{#each SIGN_STEPS as s}
				{@const done = wizardStep > s.step}
				{@const active = wizardStep === s.step}
				<button
					type="button"
					disabled={s.step >= wizardStep}
					title={s.step >= wizardStep ? "" : `Ir al paso ${s.step}: ${s.title}`}
					class={cn(
						"flex flex-col items-center gap-1 rounded-lg border px-1 py-2 text-center transition-colors sm:px-2 sm:py-3",
						active && "border-primary bg-primary/5 ring-primary/25 ring-2",
						done && !active && "border-border bg-muted/40 hover:bg-muted/70 text-foreground",
						!done && !active && "border-border/60 opacity-55",
					)}
					onclick={() => jumpToStep(s.step)}
				>
					<span
						class={cn(
							"flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold sm:size-9 sm:text-sm",
							active && "bg-primary text-primary-foreground",
							done && !active && "bg-muted-foreground/25 text-muted-foreground",
							!done && !active && "bg-muted text-muted-foreground",
						)}
					>
						{#if done}
							<CheckIcon class="size-3.5 sm:size-4" aria-hidden="true" />
							<span class="sr-only">Completado</span>
						{:else}
							{s.step}
						{/if}
					</span>
					<span class="text-[10px] font-medium leading-tight sm:text-xs">{s.title}</span>
					<span class="text-muted-foreground hidden leading-tight sm:block sm:text-[11px]">{s.hint}</span>
				</button>
			{/each}
		</div>
		<div class="bg-muted h-1.5 overflow-hidden rounded-full" role="progressbar" aria-valuenow={wizardBarPct} aria-valuemin={0} aria-valuemax={100} aria-label="Avance del asistente">
			<div
				class="bg-primary h-full rounded-full transition-[width] duration-300 ease-out"
				style="width: {wizardBarPct}%"
			></div>
		</div>
	</nav>

	{#if !isTauriRuntime()}
		<Alert>
			<FileStackIcon class="size-4" />
			<AlertTitle>Vista limitada</AlertTitle>
			<AlertDescription>Usa la app de escritorio para elegir archivos y el lector.</AlertDescription>
		</Alert>
	{/if}

	{#if wizardStep === 1}
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">{SIGN_STEPS[0].title}</Card.Title>
				<Card.Description class="text-xs">
					{SIGN_STEPS[0].hint}. Si eliges carpeta, se incluyen subcarpetas y los PDFs aparecen en la tabla.
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="flex flex-wrap gap-2">
					<Button type="button" onclick={() => pickPdfs()} disabled={busy}>
						<FileStackIcon class="mr-2 size-4" />
						Elegir PDFs…
					</Button>
					<Button type="button" variant="secondary" onclick={() => pickFolder()} disabled={busy}>
						<FolderOpenIcon class="mr-2 size-4" />
						Elegir carpeta…
					</Button>
					<Button type="button" variant="outline" disabled={busy || paths.length === 0} onclick={() => clearPaths()}>
						Limpiar
					</Button>
				</div>
				{#if outputDirForJob && sourceMode === "folder"}
					<p class="text-muted-foreground text-xs">
						Salida:
						<code class="bg-muted ml-1 rounded px-1 py-0.5 font-mono">{outputDirForJob}</code>
					</p>
				{/if}
				{#if paths.length === 0}
					<p class="text-muted-foreground text-sm">Nada seleccionado.</p>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head class="w-10">#</Table.Head>
								<Table.Head>Ruta</Table.Head>
								<Table.Head class="w-14"></Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each paths as p, i}
								<Table.Row>
									<Table.Cell>{i + 1}</Table.Cell>
									<Table.Cell class="max-w-0 font-mono text-xs">
										<span class="block truncate" title={p}>{p}</span>
									</Table.Cell>
									<Table.Cell>
										<Button
											variant="ghost"
											size="icon-xs"
											class="text-destructive"
											onclick={() => removeAt(i)}
											aria-label="Quitar"
										>
											<Trash2Icon class="size-4" />
										</Button>
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
					<p class="text-muted-foreground text-xs">{paths.length} PDF</p>
				{/if}
				<Button
					type="button"
					disabled={busy || paths.length === 0}
					onclick={() => step1Continue()}
					aria-label={`Siguiente paso: ${SIGN_STEPS[1].title}`}
				>
					Continuar
					<span class="text-primary-foreground/85 ml-1 hidden font-normal sm:inline">
						→ {SIGN_STEPS[1].title}
					</span>
				</Button>
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 2}
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">{SIGN_STEPS[1].title}</Card.Title>
				<Card.Description class="text-xs leading-relaxed">
					Selecciona el certificado con el que quieres firmar (suele ser el del DNIe). Si acabas de enchufar el
					lector, espera unos segundos y pulsa <span class="text-foreground font-medium">Actualizar lista</span>.
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				{#if certs.length === 0}
					<p class="text-muted-foreground text-sm">
						Sin certificados todavía. Comprueba la tarjeta o el lector y pulsa «Actualizar lista».
					</p>
				{:else}
					<div class="grid gap-2">
						<Label>Certificado</Label>
						<Select.Root type="single" bind:value={certId}>
							<Select.Trigger class="w-full justify-between">
								{@const selected = certs.find((c) => c.id_hex === certId)}
								{#if selected}
									<span class="truncate font-medium">
										{getHumanNameFromDn(selected.subject_dn) || selected.label} <span class="text-muted-foreground font-normal ml-1">(DNI: {extractDniFromDn(selected.subject_dn) || "—"})</span>
									</span>
								{:else}
									<span class="text-muted-foreground">Selecciona un certificado</span>
								{/if}
							</Select.Trigger>
							<Select.Content class="w-full max-h-[300px]">
								{#each certs as c}
									<Select.Item value={c.id_hex} label={getHumanNameFromDn(c.subject_dn) || c.label || ""}>
										<div class="flex flex-col text-left py-1">
											<span class="font-medium text-base">{getHumanNameFromDn(c.subject_dn) || c.label || "(sin etiqueta)"}</span>
											<span class="text-muted-foreground text-xs">DNI: {extractDniFromDn(c.subject_dn) || "—"}</span>
										</div>
									</Select.Item>
								{/each}
							</Select.Content>
						</Select.Root>
					</div>
				{/if}
				<div class="flex flex-col gap-3">
					<div class="flex flex-wrap gap-2 items-center">
						<Button
							type="button"
							variant="outline"
							size="sm"
							class="gap-1.5"
							onclick={() => void refreshCerts()}
							disabled={busy}
						>
							<RefreshCwIcon class="size-4 shrink-0 opacity-80" aria-hidden="true" />
							Actualizar lista
						</Button>
						<Button
							type="button"
							disabled={busy || !certId.trim() || certs.length === 0}
							onclick={() => step2Continue()}
							aria-label={`Siguiente paso: ${SIGN_STEPS[2].title}`}
						>
							Continuar
							<span class="text-primary-foreground/85 ml-1 hidden font-normal sm:inline">
								→ {SIGN_STEPS[2].title}
							</span>
						</Button>
					</div>

					<details
						class="group rounded-md border border-dashed border-transparent bg-muted/10 text-muted-foreground opacity-60 transition-opacity hover:opacity-90 open:border-border/50 open:bg-muted/20 open:opacity-100 dark:open:border-border/35"
					>
						<summary
							class="cursor-pointer list-none px-2 py-1.5 text-[11px] leading-snug outline-none marker:content-none [&::-webkit-details-marker]:hidden"
						>
							<span class="opacity-75 group-open:opacity-90">
								Solo si el lector falla o fallaste el PIN varias veces…
							</span>
						</summary>
						<div class="space-y-2 border-t border-border/40 px-2 pb-2 pt-2 text-[11px] leading-relaxed">
							<p>
								Reinicia la conexión con la tarjeta solo cuando «Actualizar lista» no muestra nada y ya has
								comprobado la tarjeta. No lo uses en circunstancias normales.
							</p>
							<Button
								type="button"
								variant="outline"
								size="sm"
								class="h-8 text-xs"
								disabled={busy}
								onclick={() => confirmResetReader()}
							>
								Reconectar lector
							</Button>
						</div>
					</details>
				</div>
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 3}
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">{SIGN_STEPS[2].title}</Card.Title>
				<Card.Description class="text-xs">{SIGN_STEPS[2].hint}. No se guarda en disco.</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="grid max-w-md gap-2">
					<Label for="pin-wizard">PIN</Label>
					<Input
						id="pin-wizard"
						type="password"
						autocomplete="off"
						bind:value={pin}
						placeholder="PIN"
						class={pinError ? "border-destructive focus-visible:ring-destructive" : ""}
						oninput={() => { pinError = null; }}
						onkeydown={(e) => {
							if (e.key === "Enter") {
								e.preventDefault();
								pinStepContinue();
							}
						}}
					/>
					{#if pinError}
						<p class="text-sm font-medium text-destructive">{pinError}</p>
					{/if}
				</div>
				<Button
					type="button"
					disabled={busy || !pin.trim()}
					onclick={() => pinStepContinue()}
					aria-label={`Siguiente paso: ${SIGN_STEPS[3].title}`}
				>
					Continuar
					<span class="text-primary-foreground/85 ml-1 hidden font-normal sm:inline">
						→ {SIGN_STEPS[3].title}
					</span>
				</Button>
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 4}
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">{SIGN_STEPS[3].title}</Card.Title>
				<Card.Description class="text-xs">
					{SIGN_STEPS[3].hint}. La firma criptográfica usa tu certificado y el diseño del sello configurado; aquí solo eliges dónde colocar el recuadro visible (pequeño) en la primera página.
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<p class="text-muted-foreground text-xs leading-snug">
					Fila superior = cabecera del PDF · columnas de izquierda a derecha. Cada PDF del lote usa la misma casilla.
				</p>
				<div class="mx-auto w-full max-w-md space-y-1">
					{#each [0, 1, 2, 3, 4] as row}
						<div class="grid grid-cols-7 gap-1">
							{#each [0, 1, 2, 3, 4, 5, 6] as col}
								<button
									type="button"
									class={cn(
										"aspect-square max-h-10 rounded-md border text-[10px] font-medium transition-colors sm:max-h-11",
										sigGridCol === col && sigGridRow === row
											? "border-primary bg-primary/15 text-foreground ring-primary/30 ring-2"
											: "border-border bg-muted/30 text-muted-foreground hover:bg-muted/60",
									)}
									aria-label="Casilla columna {col + 1}, fila {row + 1}"
									aria-pressed={sigGridCol === col && sigGridRow === row}
									onclick={() => {
										sigGridCol = col;
										sigGridRow = row;
									}}
								>
									{row * SIG_GRID_COLS + col + 1}
								</button>
							{/each}
						</div>
					{/each}
				</div>
				<p class="text-muted-foreground text-xs">
					Selección: columna {sigGridCol + 1} · fila {sigGridRow + 1}
				</p>
				<Button
					type="button"
					onclick={() => placementStepContinue()}
					aria-label={`Siguiente paso: ${SIGN_STEPS[4].title}`}
				>
					Continuar
					<span class="text-primary-foreground/85 ml-1 hidden font-normal sm:inline">
						→ {SIGN_STEPS[4].title}
					</span>
				</Button>
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 5}
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">{SIGN_STEPS[4].title}</Card.Title>
				<Card.Description class="text-xs">{SIGN_STEPS[4].hint}</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4 text-sm">
				<div class="text-muted-foreground space-y-1 text-xs">
					<p>
						<span class="text-foreground font-medium">{paths.length}</span>
						archivo(s) ·
						<span class="text-foreground font-medium"
							>{certs.find((c) => c.id_hex === certId)?.label || certId}</span
						>
					</p>
					<p>
						Primera página: columna <span class="text-foreground font-medium">{sigGridCol + 1}</span>,
						fila <span class="text-foreground font-medium">{sigGridRow + 1}</span>
						<span class="text-muted-foreground"> (rejilla 7×5 · recuadro visible pequeño)</span>
					</p>
					{#if outputDirForJob}
						<p class="truncate font-mono" title={outputDirForJob}>{outputDirForJob}</p>
					{:else}
						<p><code class="bg-muted rounded px-1">*_firmado.pdf</code> junto a cada PDF</p>
					{/if}
				</div>
				<div class="flex flex-wrap gap-2">
					<Button type="button" disabled={busy || paths.length === 0 || !certId} onclick={() => submitBatch()}>
						Firmar
					</Button>
					<Button type="button" variant="outline" disabled={busy} onclick={() => cancelJob()}>Cancelar</Button>
				</div>
			</Card.Content>
		</Card.Root>
	{/if}

	{#if showProgressPanel}
		<Card.Root>
			<Card.Header class="pb-2">
				<Card.Title class="text-base">Progreso</Card.Title>
			</Card.Header>
			<Card.Content class="space-y-4">
				<Progress value={progressPct} class="h-2" />
				<ScrollArea.Root class="h-40 rounded-md border">
					<div class="p-3 font-mono text-xs leading-relaxed">
						{#if logLines.length === 0}
							<span class="text-muted-foreground">Esperando eventos…</span>
						{:else}
							{#each logLines as line}
								<div>{line}</div>
							{/each}
						{/if}
					</div>
				</ScrollArea.Root>
			</Card.Content>
		</Card.Root>
	{/if}
</div>
