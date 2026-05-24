<script lang="ts">
	import { onMount } from "svelte";
	import { open, ask } from "@tauri-apps/plugin-dialog";
	import { basename, dirname, join } from "$lib/tauri/path";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Table from "$lib/components/ui/table/index.js";

	import SigningCertPicker from "$lib/components/signing-cert-picker.svelte";
	import SignConfirmPanel from "$lib/components/sign-confirm-panel.svelte";
	import SignJobResults from "$lib/components/sign-job-results.svelte";
	import { Progress } from "$lib/components/ui/progress/index.js";
	import * as ScrollArea from "$lib/components/ui/scroll-area/index.js";
	import { Alert, AlertTitle } from "$lib/components/ui/alert/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import { page } from "$app/state";
	import { postBatchSign, LocalApiHttpError, extractJsonErrorMessage } from "$lib/api/local-api";
	import { ipcPostBatchSign, LocalBackendInvokeError } from "$lib/tauri/local-backend";
	import { subscribeProgress, type ProgressPayload } from "$lib/events/progress";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import { partitionBatchPdfPaths } from "$lib/tauri/batch-validation";
	import { enumeratePdfsUnderFolder } from "$lib/tauri/batch";
	import { getBatchSignIntent } from "$lib/tauri/batch-sign-intent";
	import { isPkcs11NoTokenError } from "$lib/tauri/pkcs11-errors";
	import { maybePersistPreferredModuleAfterSuccessfulBatch } from "$lib/tauri/pkcs11-ux";
	import {
		buildSignJobFileDisplayList,
		labelFromProgressPayload,
		upsertJobFileResult,
		type SignJobFileResult,
	} from "$lib/sign/job-results";
	import { getLocalApiBaseUrl } from "$lib/tauri/settings";
	import { isTauriRuntime } from "$lib/tauri/env";
	import Loader2Icon from "@lucide/svelte/icons/loader-2";
	import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
	import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
	import FileStackIcon from "@lucide/svelte/icons/files";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
	import CheckIcon from "@lucide/svelte/icons/check";
	import BanIcon from "@lucide/svelte/icons/ban";
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
		TERMINAL_BATCH_STATUSES,
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
	let jobFileResults = $state<SignJobFileResult[]>([]);
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

	const activeJobItem = $derived(
		batchQueue.activeBatchJobId
			? (batchQueue.items.find((q) => q.jobId === batchQueue.activeBatchJobId) ?? null)
			: null,
	);

	/** Lote resuelto: progreso completo o estado terminal en cola (cancelado, error, finished). */
	const jobSettled = $derived(
		batchQueue.activeBatchJobId !== null &&
			(progressPct >= 100 ||
				(activeJobItem !== null && TERMINAL_BATCH_STATUSES.includes(activeJobItem.status))),
	);

	/** Solo mientras el backend aún admite cancelación explícita. */
	const canCancelBatchJobStep5 = $derived(
		Boolean(
			batchQueue.activeBatchJobId &&
				!jobSettled &&
				activeJobItem &&
				["preparing", "queued", "running"].includes(activeJobItem.status),
		),
	);

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

	/** Tras un lote completado sin errores en el log, persistir middleware preferido si el usuario no definió uno. */
	$effect(() => {
		if (!isTauriRuntime()) return;
		if (!jobSettled) {
			preferredLearnedForFinishedBatch = false;
			return;
		}
		if (activeJobItem?.status === "cancelled" || activeJobItem?.status === "error") return;
		if (jobLogHasErrors) return;
		if (preferredLearnedForFinishedBatch) return;
		preferredLearnedForFinishedBatch = true;
		void maybePersistPreferredModuleAfterSuccessfulBatch();
	});

	const resultStepSigning = $derived(
		wizardStep === 5 && !jobSettled && (submitInFlight || busy || batchQueue.activeBatchJobId !== null),
	);

	const wizardBarPct = $derived(Math.round((wizardStep / TOTAL_STEPS) * 100));

	const selectedCert = $derived(certs.find((c) => c.id_hex === certId) ?? null);

	const pinRequired = $derived(pkcs11.pinRequiredInApp(selectedCert));

	const jobFileDisplayList = $derived(
		buildSignJobFileDisplayList(paths, jobFileResults, { signing: resultStepSigning }),
	);

	function isStepNavDisabled(stepNum: number): boolean {
		if (stepNum === wizardStep) return true;
		if (stepHistoryLocked) return true;
		if (stepNum < wizardStep) return false;
		if (stepNum === 5 && jobSettled) return false;
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
		jobFileResults = [];
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

	/** Cierra la conexión actual con el lector y vuelve a enumerar los certificados disponibles. */
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
		if (step > wizardStep && !(step === 5 && jobSettled)) return;
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
		if (pinRequired && !pin.trim()) {
			toast.error("Introduce el PIN de tu DNIe o tarjeta para firmar.");
			pinError = "Introduce tu PIN.";
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

		if (isTauriRuntime() && pinRequired) {
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
						"El PIN está bloqueado por varios intentos fallidos. Sigue las instrucciones de tu DNIe o de tu tarjeta para desbloquearlo (suele requerir el código PUK del emisor).";
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
		jobFileResults = [];
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
			const res = isTauriRuntime()
				? await ipcPostBatchSign(body)
				: await postBatchSign(body, apiBase);
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
			if (e instanceof LocalBackendInvokeError) {
				const detail = e.detail || e.code;
				if (detail.toLowerCase().includes("demasiado grande") || detail.includes("50 MiB")) {
					toast.error(
						"Uno o más PDF superan 50 MiB. Reduce el tamaño, divide el documento o quítalo del lote.",
					);
				} else if (
					detail.toLowerCase().includes("pin") ||
					detail.toLowerCase().includes("token") ||
					detail.toLowerCase().includes("sesión")
				) {
					pinError =
						"No hemos podido firmar con ese PIN. Revisa el PIN o vuelve a conectar el lector con la tarjeta dentro.";
					toast.error(pinError);
				} else {
					toast.error(detail);
				}
			} else if (e instanceof LocalApiHttpError) {
				const detail = extractJsonErrorMessage(e.body) ?? e.message;
				if (e.status === 400 && detail.includes("demasiado grande")) {
					toast.error(
						"Uno o más PDF superan 50 MiB. Reduce el tamaño, divide el documento o quítalo del lote.",
					);
				} else if (e.status === 401) {
					pinError =
						"No hemos podido firmar con ese PIN. Revisa el PIN o vuelve a conectar el lector con la tarjeta dentro.";
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
					jobFileResults = upsertJobFileResult(jobFileResults, {
						index: p.actual,
						label: labelFromProgressPayload(p),
						inputPath: p.path,
						outputPath: p.output_path,
						error: p.error,
					});
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
		class="border-border/60 bg-background/92 supports-backdrop-filter:bg-background/85 sticky top-0 z-30 -mx-5 mb-3 flex flex-wrap items-center justify-end gap-2 border-y px-5 py-1.5 backdrop-blur-sm md:-mx-6 md:px-6"
		aria-label="Acciones del paso actual"
	>
		<div class="flex w-full shrink-0 flex-wrap items-center justify-end gap-2">
			{#if wizardStep === 5}
				{#if canCancelBatchJobStep5}
					<Button type="button" variant="outline" size="sm" onclick={() => cancelJob()}>Cancelar</Button>
				{:else if batchQueue.activeBatchJobId && !jobSettled && activeJobItem?.status === "cancelling"}
					<Badge variant="secondary" class="mr-auto h-6 text-[10px]">Cancelando…</Badge>
				{/if}
				<Button
					type="button"
					size="sm"
					disabled={!jobSettled}
					onclick={() => startNewSigningRound()}
				>
					Nuevo lote
				</Button>
			{:else if wizardStep === 4}
				<Button
					type="button"
					variant="outline"
					size="sm"
					class="mr-auto"
					disabled={busy || !batchQueue.activeBatchJobId}
					onclick={() => cancelJob()}
				>
					Cancelar
				</Button>
			{:else if wizardStep === 3}
				<Button
					type="button"
					variant="outline"
					size="sm"
					class="mr-auto h-8 text-xs"
					disabled={busy}
					onclick={() => confirmResetReader()}
				>
					Reconectar lector
				</Button>
				<Button
					type="button"
					size="sm"
					class="gap-1"
					disabled={busy || !certId.trim() || certs.length === 0}
					onclick={() => step3CertContinue()}
					aria-label={`Siguiente paso: ${SIGN_STEPS[3].title}`}
				>
					Continuar
					<ChevronRightIcon class="size-4 opacity-90" aria-hidden="true" />
				</Button>
			{:else if wizardStep === 2}
				<Button
					type="button"
					size="sm"
					class="gap-1"
					onclick={() => step2PlacementContinue()}
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
			<AlertTitle class="text-sm">Requiere la app de escritorio</AlertTitle>
		</Alert>
	{/if}

	{#if wizardStep === 1}
		<Card.Root size="sm">
			<Card.Header class="pb-1">
				<Card.Title class="text-sm font-medium">Archivos</Card.Title>
			</Card.Header>
			<Card.Content class="space-y-2 pt-0 pb-3">
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
				{#if paths.length > 0}
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
			<Card.Header class="flex flex-row items-center justify-between gap-2 space-y-0 pb-1">
				<Card.Title class="text-sm font-medium">Sello</Card.Title>
				<Badge variant="outline" class="h-5 font-mono text-[10px] tabular-nums">
					{sigGridCol + 1}·{sigGridRow + 1}
				</Badge>
			</Card.Header>
			<Card.Content class="flex justify-center pt-0 pb-3">
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
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 3}
		<Card.Root size="sm">
			<Card.Header class="flex flex-row flex-wrap items-center justify-between gap-2 space-y-0 pb-1">
				<Card.Title class="text-sm font-medium">Certificado</Card.Title>
				<Button
					type="button"
					variant="outline"
					size="sm"
					class="h-8 shrink-0 gap-1 text-xs"
					disabled={busy}
					onclick={() => refreshCerts()}
				>
					Actualizar
				</Button>
			</Card.Header>
			<Card.Content class="pt-0 pb-3">
				<SigningCertPicker
					{certs}
					bind:certId
					{busy}
					slotsWithToken={slotsWithTokenCount}
					helpVariant="brief"
					showDedupeNote={false}
					compact
				/>
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 4}
		<SignConfirmPanel
				pathCount={paths.length}
				{selectedCert}
				{sigGridCol}
				{sigGridRow}
				{outputDirForJob}
				bind:pin
				bind:pinError
				bind:pinVisible
				{busy}
				{submitInFlight}
				onSubmit={() => submitBatch()}
			/>
	{/if}

	{#if wizardStep === 5}
		<Card.Root size="sm" class="overflow-hidden">
			<Card.Header class="pb-2">
				{#if resultStepSigning}
					<div class="flex items-center gap-3">
						<div
							class="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary"
						>
							<Loader2Icon class="size-5 animate-spin" aria-hidden="true" />
						</div>
						<Card.Title class="text-base font-semibold">Firmando</Card.Title>
						<Badge variant="secondary" class="h-5 text-[10px]">{progressPct}%</Badge>
					</div>
				{:else if activeJobItem?.status === "cancelled"}
					<div class="flex items-center gap-3">
						<div
							class="bg-muted/40 flex size-10 shrink-0 items-center justify-center rounded-full text-muted-foreground"
						>
							<BanIcon class="size-5" aria-hidden="true" />
						</div>
						<Card.Title class="text-base font-semibold">Cancelado</Card.Title>
					</div>
				{:else if activeJobItem?.status === "error"}
					<div class="flex items-center gap-3">
						<div
							class="flex size-10 shrink-0 items-center justify-center rounded-full bg-destructive/15 text-destructive"
						>
							<TriangleAlertIcon class="size-5" aria-hidden="true" />
						</div>
						<Card.Title class="text-base font-semibold">Error</Card.Title>
					</div>
				{:else if jobSettled && jobLogHasErrors}
					<div class="flex items-center gap-3">
						<div
							class="flex size-10 shrink-0 items-center justify-center rounded-full bg-amber-500/15 text-amber-700 dark:text-amber-400"
						>
							<TriangleAlertIcon class="size-5" aria-hidden="true" />
						</div>
						<Card.Title class="text-base font-semibold">Con avisos</Card.Title>
					</div>
				{:else if jobSettled}
					<div class="flex items-center gap-3">
						<div
							class="flex size-10 shrink-0 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
						>
							<CircleCheckIcon class="size-5" aria-hidden="true" />
						</div>
						<Card.Title class="text-base font-semibold">Listo</Card.Title>
						{#if progressSnapshot}
							<Badge variant="outline" class="h-5 text-[10px] tabular-nums">
								{progressSnapshot.actual}/{progressSnapshot.total}
							</Badge>
						{/if}
					</div>
				{/if}
			</Card.Header>
			<Card.Content class="space-y-3 pt-0">
				<SignJobResults
					items={jobFileDisplayList}
					{outputDirForJob}
					jobSettled={jobSettled}
				/>
				{#if resultStepSigning}
					<div class="space-y-2">
						{#if progressSubtitle}
							<p class="text-muted-foreground truncate text-xs" title={progressSubtitle}>{progressSubtitle}</p>
						{/if}
						<Progress value={progressPct} max={100} class="h-2 rounded-full" />
						{#if progressSnapshot}
							<p class="text-muted-foreground text-right text-[11px] tabular-nums">
								{progressSnapshot.actual}/{progressSnapshot.total}
							</p>
						{/if}
					</div>
				{/if}
				<ScrollArea.Root
					bind:viewportRef={logViewportEl}
					class="bg-muted/25 dark:bg-muted/15 h-52 rounded-lg border shadow-inner sm:h-60"
				>
					<div class="space-y-0 p-3 font-mono text-[11px] leading-relaxed">
						{#if logLines.length === 0}
							<p class="text-muted-foreground py-6 text-center text-xs">—</p>
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
