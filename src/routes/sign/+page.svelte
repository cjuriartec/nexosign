<script lang="ts">
	import { onMount } from "svelte";
	import { open, ask } from "@tauri-apps/plugin-dialog";
	import { basename, dirname, join } from "$lib/tauri/path";
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
	import { Badge } from "$lib/components/ui/badge/index.js";
	import { page } from "$app/state";
	import { postBatchSign, LocalApiHttpError, extractJsonErrorMessage } from "$lib/api/local-api";
	import { subscribeProgress, type ProgressPayload } from "$lib/events/progress";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import { partitionBatchPdfPaths } from "$lib/tauri/batch-validation";
	import { enumeratePdfsUnderFolder } from "$lib/tauri/batch";
	import { getBatchSignIntent } from "$lib/tauri/batch-sign-intent";
	import { isPkcs11NoTokenError } from "$lib/tauri/pkcs11-errors";
	import {
		PKCS11_CERT_POLL_MS,
		emptySigningCertsHelp,
		maybePersistPreferredModuleAfterSuccessfulBatch,
	} from "$lib/tauri/pkcs11-ux";
	import { getLocalApiBaseUrl } from "$lib/tauri/settings";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { getHumanNameFromDn, extractDniFromDn } from "$lib/signature-appearance";
	import EyeIcon from "@lucide/svelte/icons/eye";
	import EyeOffIcon from "@lucide/svelte/icons/eye-off";
	import Loader2Icon from "@lucide/svelte/icons/loader-2";
	import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
	import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import FileStackIcon from "@lucide/svelte/icons/files";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
	import CheckIcon from "@lucide/svelte/icons/check";
	import { cn } from "$lib/utils.js";
	import BatchQueuePanel from "$lib/components/batch-queue-panel.svelte";
	import { cancelActiveBatchJob } from "$lib/batch/cancel-active-batch";
	import {
		batchQueue,
		clearActiveBatchJobOnly,
		completeIntentQueueItem,
		intentDetachWizard,
		setIntentActiveRequestId,
		upsertIntentQueueItem,
		prependBatchQueueItem,
		replaceQueueJobId,
		setActiveBatchJobId,
		upsertBatchQueueItem,
		type BatchQueueItem,
	} from "$lib/stores/batch-queue.svelte";
	import { SIGN_STEPS, TOTAL_STEPS, SIG_GRID_COLS, SIG_GRID_ROWS } from "$lib/sign/constants";
	import { pdfBasenameFromPath } from "$lib/sign/path-util";
	import { stripIntentQueryFromBrowser } from "$lib/sign/intent-url";
	import { buildBatchSignBodyFromWizard } from "$lib/sign/build-batch-sign-body";

	let paths = $state<string[]>([]);
	/** Origen del lote actual: archivos sueltos vs carpeta (salida agrupada). */
	let sourceMode = $state<"files" | "folder" | null>(null);
	let folderPath = $state<string | null>(null);
	/** Directorio absoluto `{padre}/{nombre}_firmados` cuando sourceMode === folder */
	let outputDirForJob = $state<string | null>(null);

	let certs = $state<SigningCertSummary[]>([]);
	/** Slots con token presente (para mensajes si la lista de firma está vacía). */
	let slotsWithTokenCount = $state(0);
	let certId = $state("");
	let pin = $state("");
	let pinVisible = $state(false);
	let pinError = $state<string | null>(null);
	let apiBase = $state("");
	let busy = $state(false);

	/** 1 archivos · 2 ubicación · 3 certificado · 4 confirmar + PIN */
	let wizardStep = $state(1);

	let sigGridCol = $state(1);
	let sigGridRow = $state(4);

	/** Si viene de `POST /api/v1/batch/sign/intent`, se envía al confirmar para cerrar la intención. */
	let intentRequestId = $state<string | null>(null);

	/** Invalida respuestas async viejas de `applyPendingIntent` (otro intent, «Nuevo lote», etc.). */
	let intentApplyGeneration = 0;

	const activeJobRef: { current: string | null } = { current: null };
	/** Tras pulsar "Firmar", no se vuelve a pasos anteriores hasta "Nuevo lote". */
	let stepHistoryLocked = $state(false);
	let progressPct = $state(0);
	let logLines = $state<string[]>([]);
	/** Último tick de progreso (para título y subtítulo del panel). */
	let progressSnapshot = $state<{
		actual: number;
		total: number;
		fileLabel: string;
	} | null>(null);

	let logViewportEl = $state<HTMLElement | null>(null);
	/** Evita enviar dos veces POST /batch/sign antes de que termine la petición. */
	let submitInFlight = $state(false);

	const progressSubtitle = $derived.by(() => {
		if (!progressSnapshot) {
			return batchQueue.activeBatchJobId ? "Preparando firma…" : "";
		}
		const { actual, total, fileLabel } = progressSnapshot;
		const base = `Documento ${actual} de ${total}`;
		return fileLabel ? `${base} · ${fileLabel}` : base;
	});

	const executionFinished = $derived(batchQueue.activeBatchJobId !== null && progressPct >= 100);

	function logLineTone(line: string): "muted" | "ok" | "err" {
		const lower = line.toLowerCase();
		if (
			/\berror\b/.test(lower) ||
			/\bfallo\b/.test(lower) ||
			/\bfalló\b/.test(lower) ||
			/\bincorrecto\b/.test(lower) ||
			(line.includes(" · ") && line.split(" · ").length >= 3)
		) {
			return "err";
		}
		if (/\bok\b|\bcompletad|\bfirmad/i.test(lower)) {
			return "ok";
		}
		return "muted";
	}

	const jobLogHasErrors = $derived(logLines.some((l) => logLineTone(l) === "err"));

	/** Evita llamar varias veces a persistir middleware preferido por el mismo lote terminado. */
	let preferredLearnedForFinishedBatch = $state(false);

	/** Polling ligero: refrescar certificados en pasos 1–4 si la pestaña está visible (lector recién conectado). */
	$effect(() => {
		if (!isTauriRuntime()) return;
		const poll = wizardStep >= 1 && wizardStep <= 4;
		if (!poll) return;
		const id = window.setInterval(() => {
			if (document.visibilityState !== "visible") return;
			void refreshCerts();
		}, PKCS11_CERT_POLL_MS);
		return () => window.clearInterval(id);
	});

	/** Tras un lote completado sin errores en el log, persistir middleware preferido si el usuario no definió uno. */
	$effect(() => {
		if (!isTauriRuntime()) return;
		if (!executionFinished) {
			preferredLearnedForFinishedBatch = false;
			return;
		}
		if (jobLogHasErrors) return;
		if (preferredLearnedForFinishedBatch) return;
		preferredLearnedForFinishedBatch = true;
		void maybePersistPreferredModuleAfterSuccessfulBatch();
	});

	const resultStepSigning = $derived(
		wizardStep === 5 && !executionFinished && (submitInFlight || busy || batchQueue.activeBatchJobId !== null),
	);

	const wizardBarPct = $derived(Math.round((wizardStep / TOTAL_STEPS) * 100));

	const selectedCert = $derived(certs.find((c) => c.id_hex === certId) ?? null);

	function isStepNavDisabled(stepNum: number): boolean {
		if (stepNum === wizardStep) return true;
		if (stepHistoryLocked) return true;
		if (stepNum < wizardStep) return false;
		if (stepNum === 5 && executionFinished) return false;
		return true;
	}

	function startNewSigningRound() {
		intentApplyGeneration++;
		wizardStep = 1;
		clearActiveBatchJobOnly();
		activeJobRef.current = null;
		progressPct = 0;
		progressSnapshot = null;
		logLines = [];
		pin = "";
		pinError = null;
		submitInFlight = false;
		stepHistoryLocked = false;
		clearPaths();
	}

	function pushLog(line: string) {
		logLines = [...logLines, line].slice(-120);
	}

	$effect(() => {
		const n = logLines.length;
		if (n === 0) return;
		queueMicrotask(() => {
			const el = logViewportEl;
			if (!el) return;
			el.scrollTo({
				top: el.scrollHeight,
				behavior: n <= 2 ? "auto" : "smooth",
			});
		});
	});

	async function partitionPaths(list: string[]): Promise<string[]> {
		if (!isTauriRuntime()) return list;
		if (list.length === 0) return [];
		const { accepted, rejected } = await partitionBatchPdfPaths(list);
		if (rejected.length > 0) {
			const desc = rejected.map((r) => `${r.path}: ${r.reason}`).join("\n");
			toast.error(
				rejected.length === 1
					? rejected[0].reason
					: `${rejected.length} archivo(s) no se incluyeron (revisa tamaño ≤ 50 MiB y extensión .pdf)`,
				{ description: desc },
			);
		}
		return accepted;
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
		} finally {
			slotsWithTokenCount = await pkcs11.pkcs11SlotCount().catch(() => 0);
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
		paths = await partitionPaths(unique);
		intentRequestId = null;
		intentDetachWizard();
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
			paths = await partitionPaths(pdfs);
			sourceMode = "folder";
			folderPath = sel;
			outputDirForJob = await computeFirmadosDir(sel);
			intentRequestId = null;
			intentDetachWizard();
			if (pdfs.length === 0) {
				toast.message("No hay PDFs en esa carpeta.");
			} else if (paths.length === 0) {
				toast.message("Ningún PDF válido en esa carpeta.");
			}
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	async function applyPendingIntent(intentParam: string) {
		const gen = ++intentApplyGeneration;
		// Quitar PDF/carpeta previos de inmediato; el lote debe ser solo el del intent.
		clearPaths();

		const payload = await getBatchSignIntent(intentParam);
		if (gen !== intentApplyGeneration) return;

		if (!payload) {
			toast.error("La solicitud no existe o caducó (~5 min). Abre el enlace desde la integración de nuevo.");
			return;
		}
		paths = await partitionPaths([...payload.inputs]);
		if (gen !== intentApplyGeneration) return;
		sourceMode = "files";
		folderPath = null;
		outputDirForJob = payload.outputDir ?? null;
		intentRequestId = intentParam;
		const label =
			paths.length > 0 ? `${paths.length} PDF · ${pdfBasenameFromPath(paths[0] ?? "")}` : "Pendientes";
		upsertIntentQueueItem({
			requestId: intentParam,
			label,
			fileCount: paths.length,
		});
		setIntentActiveRequestId(intentParam);
		await refreshCerts();
		if (gen !== intentApplyGeneration) return;
		wizardStep = 1;
		toast.message("Cargado.");
		if (typeof window !== "undefined") {
			stripIntentQueryFromBrowser();
		}
	}

	function clearPaths() {
		paths = [];
		sourceMode = null;
		folderPath = null;
		outputDirForJob = null;
		intentRequestId = null;
		intentDetachWizard();
	}

	function removeAt(i: number) {
		paths = paths.filter((_, idx) => idx !== i);
		if (paths.length === 0) clearPaths();
	}

	async function step1Continue() {
		if (paths.length === 0) return;
		wizardStep = 2;
	}

	async function step2PlacementContinue() {
		wizardStep = 3;
		void refreshCerts();
	}

	async function step3CertContinue() {
		if (!certId.trim()) {
			toast.error("Selecciona un certificado.");
			return;
		}
		pinError = null;
		wizardStep = 4;
	}

	function goBack() {
		if (stepHistoryLocked) return;
		if (wizardStep <= 1) return;
		wizardStep -= 1;
	}

	function jumpToStep(step: number) {
		if (stepHistoryLocked) return;
		if (step < 1 || step > TOTAL_STEPS) return;
		if (step === wizardStep) return;
		if (step > wizardStep && !(step === 5 && executionFinished)) return;
		wizardStep = step;
	}

	async function submitBatch() {
		if (submitInFlight) return;
		if (!certId.trim()) {
			toast.error("Selecciona un certificado.");
			return;
		}
		if (paths.length === 0) {
			toast.error("No hay PDFs.");
			return;
		}
		if (!pin.trim()) {
			toast.error("Indica el PIN del token para firmar.");
			pinError = "Introduce el PIN del token.";
			return;
		}

		submitInFlight = true;
		stepHistoryLocked = true;
		const pendingQueueId = `pending-${Date.now()}`;
		const pendingQueueItem: BatchQueueItem = {
			jobId: pendingQueueId,
			status: "preparing",
			label: `${paths.length} PDF(s)`,
			progressPct: 0,
			createdAt: Date.now(),
		};
		prependBatchQueueItem(pendingQueueItem);

		if (isTauriRuntime()) {
			try {
				await pkcs11.pkcs11VerifyPin(pin.trim(), certId.trim());
				toast.success("PIN correcto");
			} catch (e) {
				submitInFlight = false;
				upsertBatchQueueItem(pendingQueueId, { status: "error" });
				const msg = String(e);
				if (msg.includes("PIN incorrecto")) {
					pinError = "PIN incorrecto.";
					toast.error("PIN incorrecto.");
				} else if (msg.includes("PIN bloqueado")) {
					pinError =
						"El PIN está bloqueado por demasiados intentos fallidos. Desbloquea el token según las instrucciones del fabricante.";
					toast.error(pinError);
				} else {
					pinError = msg;
					toast.error(msg);
				}
				return;
			}
		}

		busy = true;
		pinError = null;
		logLines = [];
		progressPct = 0;
		progressSnapshot = null;
		setActiveBatchJobId(null);
		activeJobRef.current = null;
		wizardStep = 5;

		try {
			const body = await buildBatchSignBodyFromWizard({
				certIdHex: certId,
				paths,
				pin,
				sigGridCol,
				sigGridRow,
				outputDirForJob,
				intentRequestId,
				selectedCert: certs.find((c) => c.id_hex === certId.trim()) ?? null,
			});
			const intentToFinish = intentRequestId;
			const res = await postBatchSign(body, apiBase);
			if (intentToFinish) {
				completeIntentQueueItem(intentToFinish);
			}
			setActiveBatchJobId(res.job_id);
			activeJobRef.current = res.job_id;
			intentRequestId = null;
			replaceQueueJobId(pendingQueueId, res.job_id, {
				status: "running",
				label: pdfBasenameFromPath(paths[0] ?? "Lote"),
			});
		} catch (e) {
			wizardStep = 4;
			upsertBatchQueueItem(pendingQueueId, { status: "error" });
			if (e instanceof LocalApiHttpError) {
				const detail = extractJsonErrorMessage(e.body) ?? e.message;
				if (e.status === 400 && detail.includes("demasiado grande")) {
					toast.error(
						"Uno o más PDF superan 50 MiB. Reduce el tamaño, divide el documento o quítalo del lote.",
					);
				} else if (e.status === 401) {
					pinError =
						"No se pudo desbloquear el token con ese PIN o la sesión del lector está bloqueada.";
					toast.error(pinError);
				} else if (e.status === 403 && String(detail).toLowerCase().includes("origin")) {
					toast.error(
						"Origen no autorizado para la API local. Añade este origen en Ajustes → Orígenes permitidos.",
					);
				} else {
					toast.error(detail);
				}
			} else {
				toast.error(String(e));
			}
		} finally {
			busy = false;
			submitInFlight = false;
		}
	}

	async function cancelJob() {
		await cancelActiveBatchJob();
	}

	$effect(() => {
		const q = page.url.searchParams.get("intent");
		if (!isTauriRuntime() || !q) return;
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
					const jid =
						(typeof p.job_id === "string" && p.job_id.length > 0
							? p.job_id
							: typeof p.jobId === "string" && p.jobId.length > 0
								? p.jobId
								: "") || "";
					if (!jid) return;
					const queueHas = batchQueue.items.some((q) => q.jobId === jid);
					const activeMatches = activeJobRef.current === jid;
					if (!queueHas && !activeMatches) return;
					const total = Math.max(1, p.total);
					progressPct = Math.min(100, Math.round((100 * p.actual) / total));
					const tail = p.nombre_archivo || p.path || "";
					const baseLabel = tail.replace(/^.*[/\\]/, "") || tail;
					progressSnapshot = {
						actual: p.actual,
						total: p.total,
						fileLabel: baseLabel,
					};
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
		activeJobRef.current = batchQueue.activeBatchJobId;
	});
</script>

<svelte:head>
	<title>Firmar — NexoSign</title>
</svelte:head>

<div class="space-y-4 pb-6">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<h1 class="text-2xl font-semibold tracking-tight">Firmar</h1>
		{#if wizardStep > 1}
			<Button variant="outline" size="sm" onclick={() => goBack()} class="gap-1" disabled={stepHistoryLocked}>
				<ChevronLeftIcon class="size-4" />
				Atrás
			</Button>
		{/if}
	</div>

	<BatchQueuePanel showWizardLockHint activeWorkOnly />

	<nav class="space-y-2" aria-label="Pasos del asistente de firma">
		<div class="grid grid-cols-5 gap-0.5 sm:gap-1">
			{#each SIGN_STEPS as s}
				{@const done = wizardStep > s.step}
				{@const active = wizardStep === s.step}
				<button
					type="button"
					disabled={isStepNavDisabled(s.step)}
					title={`Paso ${s.step}: ${s.title}`}
					class={cn(
						"flex flex-col items-center gap-0.5 rounded-md border px-1 py-1.5 text-center transition-colors sm:px-2 sm:py-2",
						active && "border-primary bg-primary/5 ring-primary/25 ring-2",
						done && !active && "border-border bg-muted/40 hover:bg-muted/70 text-foreground",
						!done && !active && "border-border/60 opacity-55",
					)}
					onclick={() => jumpToStep(s.step)}
				>
					<span
						class={cn(
							"flex size-6 shrink-0 items-center justify-center rounded-full text-[11px] font-semibold sm:size-8 sm:text-xs",
							active && "bg-primary text-primary-foreground",
							done && !active && "bg-muted-foreground/25 text-muted-foreground",
							!done && !active && "bg-muted text-muted-foreground",
						)}
					>
						{#if done}
							<CheckIcon class="size-3 sm:size-3.5" aria-hidden="true" />
							<span class="sr-only">Completado</span>
						{:else}
							{s.step}
						{/if}
					</span>
					<span class="text-[10px] font-medium leading-none sm:text-[11px]">{s.title}</span>
				</button>
			{/each}
		</div>
		<div class="bg-muted h-1 overflow-hidden rounded-full" role="progressbar" aria-valuenow={wizardBarPct} aria-valuemin={0} aria-valuemax={100} aria-label="Avance del asistente">
			<div
				class="bg-primary h-full rounded-full transition-[width] duration-300 ease-out"
				style="width: {wizardBarPct}%"
			></div>
		</div>
	</nav>

	<div
		class="border-border/60 bg-background/92 supports-backdrop-filter:bg-background/85 sticky top-0 z-30 -mx-5 mb-4 flex flex-wrap items-center justify-end gap-2 border-y px-5 py-2 backdrop-blur-sm md:-mx-6 md:px-6"
		aria-label="Acciones del paso actual"
	>
		{#if wizardStep === 1 && paths.length === 0}
			<p class="text-muted-foreground mr-auto max-w-[min(100%,18rem)] text-[11px] leading-snug">
				Selecciona PDF o carpeta.
			</p>
		{/if}
		<div class="flex shrink-0 flex-wrap items-center justify-end gap-2">
			{#if wizardStep === 5}
				{#if batchQueue.activeBatchJobId && !executionFinished}
					<Button type="button" variant="outline" size="sm" onclick={() => cancelJob()}>Cancelar cola</Button>
				{/if}
				<Button
					type="button"
					size="sm"
					disabled={!executionFinished}
					onclick={() => startNewSigningRound()}
				>
					Nuevo lote
				</Button>
			{:else if wizardStep === 4}
				<Button
					type="button"
					variant="outline"
					size="sm"
					disabled={busy || !batchQueue.activeBatchJobId}
					onclick={() => cancelJob()}
				>
					Cancelar cola
				</Button>
				<Button
					type="button"
					size="sm"
					disabled={busy || submitInFlight || paths.length === 0 || !certId.trim() || !pin.trim()}
					onclick={() => void submitBatch()}
					class="gap-1"
				>
					Firmar
					<ChevronRightIcon class="size-4 opacity-90" aria-hidden="true" />
				</Button>
			{:else if wizardStep === 3}
				<Button
					type="button"
					size="sm"
					disabled={busy || !certId.trim() || certs.length === 0}
					onclick={() => step3CertContinue()}
					class="gap-1"
					aria-label={`Siguiente paso: ${SIGN_STEPS[3].title}`}
				>
					Continuar
					<ChevronRightIcon class="size-4 opacity-90" aria-hidden="true" />
				</Button>
			{:else if wizardStep === 2}
				<Button
					type="button"
					size="sm"
					onclick={() => step2PlacementContinue()}
					class="gap-1"
					aria-label={`Siguiente paso: ${SIGN_STEPS[2].title}`}
				>
					Continuar
					<ChevronRightIcon class="size-4 opacity-90" aria-hidden="true" />
				</Button>
			{:else if wizardStep === 1}
				<Button
					type="button"
					size="sm"
					disabled={busy || paths.length === 0}
					onclick={() => step1Continue()}
					class="gap-1"
					aria-label={`Siguiente paso: ${SIGN_STEPS[1].title}`}
				>
					Continuar
					<ChevronRightIcon class="size-4 opacity-90" aria-hidden="true" />
				</Button>
			{/if}
		</div>
	</div>

	{#if !isTauriRuntime()}
		<Alert class="py-2">
			<FileStackIcon class="size-4" />
			<AlertTitle class="text-sm">Solo en la app de escritorio</AlertTitle>
			<AlertDescription class="text-xs">Archivos, lector y token requieren Tauri.</AlertDescription>
		</Alert>
	{/if}

	{#if wizardStep === 1}
		<Card.Root size="sm">
			<Card.Header class="pb-2">
				<Card.Title class="text-sm font-medium">Archivos</Card.Title>
				<Card.Description class="text-xs">
					Incluye subcarpetas al elegir carpeta. Salida: carpeta hermana <code class="bg-muted rounded px-1 font-mono text-[11px]">…_firmados</code>.
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-3 pt-0">
				<div class="flex flex-wrap gap-2">
					<Button type="button" size="sm" onclick={() => pickPdfs()} disabled={busy}>
						<FileStackIcon class="mr-1.5 size-4" />
						PDF…
					</Button>
					<Button type="button" variant="secondary" size="sm" onclick={() => pickFolder()} disabled={busy}>
						<FolderOpenIcon class="mr-1.5 size-4" />
						Carpeta…
					</Button>
					<Button type="button" variant="outline" size="sm" disabled={busy || paths.length === 0} onclick={() => clearPaths()}>
						Limpiar
					</Button>
				</div>
				{#if outputDirForJob && sourceMode === "folder"}
					<p class="text-muted-foreground truncate font-mono text-[11px]" title={outputDirForJob}>{outputDirForJob}</p>
				{/if}
				{#if paths.length === 0}
					<p class="text-muted-foreground text-xs">Sin archivos.</p>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head class="w-8 py-2 text-xs">#</Table.Head>
								<Table.Head class="py-2 text-xs">Ruta ({paths.length})</Table.Head>
								<Table.Head class="w-10 py-2"></Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each paths as p, i}
								<Table.Row class="[&_td]:py-1.5">
									<Table.Cell class="text-muted-foreground text-xs">{i + 1}</Table.Cell>
									<Table.Cell class="max-w-0 font-mono text-[11px]">
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
				{/if}
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 2}
		<Card.Root size="sm">
			<Card.Header class="pb-2">
				<Card.Title class="text-sm font-medium">Ubicación del sello</Card.Title>
				<Card.Description class="text-xs">1.ª página, rejilla 3×5 (fila 1 = cabecera del PDF).</Card.Description>
			</Card.Header>
			<Card.Content class="flex flex-col items-center gap-2 pt-0 sm:flex-row sm:items-start sm:justify-center sm:gap-6">
				<div class="w-fit overflow-hidden rounded-md border border-border bg-muted/25">
					{#each [0, 1, 2, 3, 4] as row}
						<div class="flex border-b border-border/70 last:border-b-0">
							{#each [0, 1, 2] as col}
								<button
									type="button"
									class={cn(
										"flex h-8 w-8 shrink-0 items-center justify-center border-r border-border/70 text-[10px] font-medium transition-colors last:border-r-0 sm:h-9 sm:w-9",
										sigGridCol === col && sigGridRow === row
											? "bg-primary/20 text-foreground ring-2 ring-primary/35 ring-inset"
											: "bg-background/90 text-muted-foreground hover:bg-muted/80",
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
				<p class="text-muted-foreground text-center text-[11px] sm:min-w-32 sm:text-left">
					Columna {sigGridCol + 1}, fila {sigGridRow + 1}
				</p>
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 3}
		<Card.Root size="sm">
			<Card.Header class="pb-2">
				<Card.Title class="text-sm font-medium">Certificado</Card.Title>
				<Card.Description class="text-xs">
					La lista se actualiza sola cada pocos segundos en este paso; también puede usar Actualizar.
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-3 pt-0">
				{#if certs.length === 0}
					{@const help = emptySigningCertsHelp(slotsWithTokenCount)}
					<Alert variant={slotsWithTokenCount <= 0 ? "destructive" : "default"} class="text-left">
						<TriangleAlertIcon class="size-4" />
						<AlertTitle class="text-sm">{help.title}</AlertTitle>
						<AlertDescription class="text-xs leading-snug">{help.description}</AlertDescription>
					</Alert>
				{:else}
					<div class="grid gap-1.5">
						<Label class="text-xs">Certificado</Label>
						<Select.Root type="single" bind:value={certId}>
							<Select.Trigger class="h-9 w-full justify-between">
								{@const selected = certs.find((c) => c.id_hex === certId)}
								{#if selected}
									<span class="truncate text-sm font-medium">
										{getHumanNameFromDn(selected.subject_dn) || selected.label} <span class="text-muted-foreground font-normal">({extractDniFromDn(selected.subject_dn) || "—"})</span>
									</span>
								{:else}
									<span class="text-muted-foreground text-sm">Elegir…</span>
								{/if}
							</Select.Trigger>
							<Select.Content class="max-h-[280px]">
								{#each certs as c}
									<Select.Item value={c.id_hex} label={getHumanNameFromDn(c.subject_dn) || c.label || ""}>
										<div class="flex flex-col py-0.5 text-left">
											<span class="text-sm font-medium">{getHumanNameFromDn(c.subject_dn) || c.label || "(sin etiqueta)"}</span>
											<span class="text-muted-foreground text-[11px]">{extractDniFromDn(c.subject_dn) || "—"}</span>
										</div>
									</Select.Item>
								{/each}
							</Select.Content>
						</Select.Root>
					</div>
				{/if}
				<Button
					type="button"
					variant="outline"
					size="sm"
					class="gap-1.5"
					onclick={() => void refreshCerts()}
					disabled={busy}
				>
					<RefreshCwIcon class="size-4 opacity-80" aria-hidden="true" />
					Actualizar
				</Button>

				<details
					class="group rounded border border-dashed border-border/40 bg-muted/15 text-[11px] text-muted-foreground open:bg-muted/25"
				>
					<summary
						class="cursor-pointer list-none px-2 py-1.5 outline-none marker:content-none [&::-webkit-details-marker]:hidden"
					>
						Problemas con el lector
					</summary>
					<div class="space-y-2 border-t border-border/40 px-2 pb-2 pt-2 leading-snug">
						<p>Solo si no aparece ningún certificado tras comprobar la tarjeta.</p>
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
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 4}
		<Card.Root size="sm" class="w-full overflow-hidden">
			<Card.Header class="border-border/50 border-b pb-3">
				<Card.Title class="text-base font-semibold">Confirmar firma</Card.Title>
				<Card.Description class="text-xs leading-relaxed">
					Comprueba el resumen e introduce el PIN del token. El PIN no se almacena.
				</Card.Description>
			</Card.Header>
			<Card.Content class="px-4 pt-4 pb-4 text-sm sm:px-5">
				<div class="mx-auto w-full max-w-xl space-y-0">
					<div
						class="border-border/60 bg-card w-full overflow-hidden rounded-lg border shadow-sm"
					>
						<p class="text-muted-foreground border-border/50 bg-muted/30 border-b px-3 py-2 text-[11px] font-medium tracking-wide uppercase">
							Resumen
						</p>
						<dl class="divide-border/40 divide-y text-xs">
							<div class="flex items-start justify-between gap-4 px-3 py-2.5">
								<dt class="text-muted-foreground shrink-0">PDF</dt>
								<dd class="text-foreground font-semibold tabular-nums">{paths.length}</dd>
							</div>
							<div class="flex items-start justify-between gap-4 px-3 py-2.5">
								<dt class="text-muted-foreground shrink-0">Firma</dt>
								<dd class="min-w-0 flex-1 text-right leading-tight">
									{#if selectedCert}
										<span class="text-foreground font-medium">
											{getHumanNameFromDn(selectedCert.subject_dn) || "Titular"}
										</span>
										{#if extractDniFromDn(selectedCert.subject_dn)}
											<span class="text-muted-foreground mt-0.5 block text-[11px]"
												>{extractDniFromDn(selectedCert.subject_dn)}</span
											>
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
										<span class="font-medium" title={outputDirForJob}>«{pdfBasenameFromPath(outputDirForJob)}»</span>
									{:else}
										<code class="bg-muted rounded px-1.5 py-0.5 font-mono text-[11px]">*_firmado.pdf</code>
									{/if}
								</dd>
							</div>
						</dl>
						<div class="border-border/50 bg-muted/25 border-t">
							<div class="px-3 pb-3 pt-3">
								<Label for="pin-confirm" class="text-xs font-medium">PIN del token</Label>
								<div class="relative mt-1.5">
									<Input
										id="pin-confirm"
										type={pinVisible ? "text" : "password"}
										autocomplete="off"
										bind:value={pin}
										placeholder="PIN"
										class={cn(
											"h-10 w-full pr-10",
											pinError ? "border-destructive focus-visible:ring-destructive" : "",
										)}
										oninput={() => {
											pinError = null;
										}}
										onkeydown={(e) => {
											if (e.key === "Enter") {
												e.preventDefault();
												void submitBatch();
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
									<p class="mt-2 text-xs font-medium text-destructive">{pinError}</p>
								{/if}
							</div>
						</div>
					</div>

				</div>
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 5}
		<Card.Root size="sm" class="overflow-hidden">
			<Card.Header class="pb-3">
				{#if resultStepSigning}
					<div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:gap-4">
						<div
							class="flex size-14 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary"
						>
							<Loader2Icon class="size-7 animate-spin" aria-hidden="true" />
						</div>
						<div class="min-w-0 flex-1 space-y-1">
							<div class="flex flex-wrap items-center gap-2">
								<Card.Title class="text-lg font-semibold tracking-tight">Firma en curso</Card.Title>
								<Badge variant="secondary" class="h-5 text-[10px]">En progreso</Badge>
							</div>
							<Card.Description class="text-xs leading-relaxed">
								Encolando y firmando los PDF; el registro se actualiza al avanzar cada archivo.
							</Card.Description>
							{#if batchQueue.activeBatchJobId}
								<p class="text-muted-foreground pt-1 font-mono text-[10px]">
									ID trabajo: <span title={batchQueue.activeBatchJobId}>{batchQueue.activeBatchJobId}</span>
								</p>
							{/if}
						</div>
					</div>
				{:else if executionFinished && jobLogHasErrors}
					<div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:gap-4">
						<div
							class="flex size-12 shrink-0 items-center justify-center rounded-full bg-amber-500/15 text-amber-700 dark:text-amber-400"
						>
							<TriangleAlertIcon class="size-7" aria-hidden="true" />
						</div>
						<div class="min-w-0 space-y-1">
							<Card.Title class="text-base font-semibold">Proceso terminado con incidencias</Card.Title>
							<Card.Description class="text-xs leading-relaxed">
								Revisa el registro: algunos archivos pueden haber fallado o mostrar avisos.
							</Card.Description>
						</div>
					</div>
				{:else if executionFinished}
					<div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:gap-4">
						<div
							class="flex size-14 shrink-0 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
						>
							<CircleCheckIcon class="size-8" aria-hidden="true" />
						</div>
						<div class="min-w-0 space-y-0.5">
							<Card.Title class="text-lg font-semibold tracking-tight">Firma completada</Card.Title>
							<Card.Description class="text-xs">
								{#if progressSnapshot}
									{progressSnapshot.total} documento(s) procesado(s).
								{:else}
									Lote finalizado correctamente.
								{/if}
							</Card.Description>
						</div>
					</div>
				{/if}
				{#if batchQueue.activeBatchJobId && executionFinished}
					<p class="text-muted-foreground pt-2 font-mono text-[10px]">
						ID trabajo: <span title={batchQueue.activeBatchJobId}>{batchQueue.activeBatchJobId}</span>
					</p>
				{/if}
			</Card.Header>
			<Card.Content class="space-y-3 pt-0">
				{#if resultStepSigning}
					<div class="border-border/50 bg-muted/20 space-y-2 rounded-lg border px-3 py-3">
						{#if progressSubtitle}
							<p class="text-muted-foreground truncate text-xs">{progressSubtitle}</p>
						{:else}
							<p class="text-muted-foreground text-xs">Preparando petición…</p>
						{/if}
						<div class="flex items-end justify-between gap-2">
							<div class="flex items-baseline gap-1">
								<span class="text-foreground tabular-nums text-2xl font-semibold">{progressPct}</span>
								<span class="text-muted-foreground text-xs">%</span>
							</div>
							{#if progressSnapshot}
								<span class="text-muted-foreground text-xs tabular-nums"
									>{progressSnapshot.actual}/{progressSnapshot.total}</span
								>
							{/if}
						</div>
						<Progress value={progressPct} max={100} class="h-2 rounded-full" />
					</div>
				{/if}
				<p class="text-muted-foreground text-[11px] font-medium uppercase tracking-wide">Registro del proceso</p>
				<ScrollArea.Root
					bind:viewportRef={logViewportEl}
					class="bg-muted/25 dark:bg-muted/15 h-52 rounded-lg border shadow-inner sm:h-60"
				>
					<div class="space-y-0 p-3 font-mono text-[11px] leading-relaxed">
						{#if logLines.length === 0}
							<p class="text-muted-foreground py-8 text-center text-xs">
								{#if resultStepSigning}
									Esperando eventos del firmador…
								{:else}
									No hay líneas de registro.
								{/if}
							</p>
						{:else}
							{#each logLines as line, i}
								{@const tone = logLineTone(line)}
								<div
									class={cn(
										"border-border/30 rounded-md border-l-2 py-1.5 pl-2 pr-1",
										i === logLines.length - 1 ? "border-primary bg-primary/6" : "border-transparent",
										tone === "err" && "text-destructive",
										tone === "ok" && "text-emerald-700 dark:text-emerald-400",
										tone === "muted" && "text-foreground/90",
									)}
								>
									{line}
								</div>
							{/each}
						{/if}
					</div>
				</ScrollArea.Root>
			</Card.Content>
		</Card.Root>
	{/if}
</div>
