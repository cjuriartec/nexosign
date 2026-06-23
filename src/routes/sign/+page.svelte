<script lang="ts">
	import { onMount } from "svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import { basename, dirname, join } from "$lib/tauri/path";
	import { toast } from "svelte-sonner";
	import { toastFail, toastInfo, toastWarn } from "$lib/ui/app-toast";
	import { toastPdfRejections } from "$lib/ui/pdf-rejection-toast";
	import { Button } from "$lib/components/ui/button/index.js";

	import SignComposePanel from "$lib/components/sign-compose-panel.svelte";
	import SignJobResults from "$lib/components/sign-job-results.svelte";
	import SignWizardStepper from "$lib/components/sign-wizard-stepper.svelte";
	import { Progress } from "$lib/components/ui/progress/index.js";
	import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import { page } from "$app/state";
	import {
		fetchBatchJobStatus,
		postBatchSign,
		LocalApiHttpError,
		extractJsonErrorMessage,
	} from "$lib/api/local-api";
	import { ipcFetchBatchJobStatus, ipcPostBatchSign, LocalBackendInvokeError } from "$lib/tauri/local-backend";
	import { subscribeProgress, type ProgressPayload } from "$lib/events/progress";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import { partitionBatchPdfPaths } from "$lib/tauri/batch-validation";
	import { enumeratePdfsUnderFolder } from "$lib/tauri/batch";
	import { getBatchSignIntent } from "$lib/tauri/batch-sign-intent";
	import { isPkcs11NoTokenError } from "$lib/tauri/pkcs11-errors";
	import {
		hasPkcs11ChipCerts,
		maybePersistPreferredModuleAfterSuccessfulBatch,
		signingCertListContextHint,
	} from "$lib/tauri/pkcs11-ux";
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
	import BanIcon from "@lucide/svelte/icons/ban";
	import PenLineIcon from "@lucide/svelte/icons/pen-line";
	import UploadIcon from "@lucide/svelte/icons/upload";
	import { listenWindowFileDrop } from "$lib/tauri/drag-drop";
	import { ingestDroppedPaths } from "$lib/sign/ingest-dropped-paths";
	import {
		loadLastSigningCertId,
		loadSignaturePlacement,
		saveLastSigningCertId,
		saveSignaturePlacement,
	} from "$lib/sign/signature-placement-prefs";
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
	import { TOTAL_STEPS } from "$lib/sign/constants";
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
	let probePin = $state("");
	let pinProbeBusy = $state(false);
	let certId = $state("");
	let pin = $state("");
	let pinVisible = $state(false);
	let pinError = $state<string | null>(null);
	let apiBase = $state("");
	let busy = $state(false);

	/** 1 preparar · 2 resultado */
	let wizardStep = $state(1);

	const savedPlacement = loadSignaturePlacement();
	let sigGridCol = $state(savedPlacement.col);
	let sigGridRow = $state(savedPlacement.row);

	let dropHover = $state(false);
	let pinModalOpen = $state(false);

	/** Si viene de `POST /api/v1/batch/sign/intent`, se envía al confirmar para cerrar la intención. */
	let intentRequestId = $state<string | null>(null);

	/** Invalida respuestas async viejas de `applyPendingIntent` (otro intent, «Nuevo lote», etc.). */
	let intentApplyGeneration = 0;

	const activeJobRef: { current: string | null } = { current: null };
	/** Tras pulsar "Firmar", no se vuelve a pasos anteriores hasta "Nuevo lote". */
	let stepHistoryLocked = $state(false);
	let progressPct = $state(0);
	let jobFileResults = $state<SignJobFileResult[]>([]);
	/** Error de lote completo (API / cola), no de un PDF concreto. */
	let batchFlowError = $state<string | null>(null);
	/** Último tick de progreso (para título y subtítulo del panel). */
	let progressSnapshot = $state<{
		actual: number;
		total: number;
		fileLabel: string;
	} | null>(null);

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

	/** Evita llamar varias veces a persistir middleware preferido por el mismo lote terminado. */
	let preferredLearnedForFinishedBatch = $state(false);

	const resultStepSigning = $derived(
		wizardStep === 2 && !jobSettled && (submitInFlight || busy || batchQueue.activeBatchJobId !== null),
	);

	const selectedCert = $derived(certs.find((c) => c.id_hex === certId) ?? null);

	const pinRequired = $derived(pkcs11.pinRequiredInApp(selectedCert));

	const listContextHint = $derived(
		isTauriRuntime() ? signingCertListContextHint(certs, slotsWithTokenCount) : null,
	);
	const showPinProbe = $derived(
		isTauriRuntime() && wizardStep === 1 && slotsWithTokenCount > 0 && !hasPkcs11ChipCerts(certs),
	);

	const canSign = $derived(
		!busy &&
			!submitInFlight &&
			paths.length > 0 &&
			!!certId.trim() &&
			certs.length > 0,
	);

	const signButtonLabel = $derived(
		paths.length === 0
			? "Firmar"
			: paths.length === 1
				? "Firmar 1 PDF"
				: `Firmar ${paths.length} PDF`,
	);

	const jobFileDisplayList = $derived(
		buildSignJobFileDisplayList(paths, jobFileResults, { signing: resultStepSigning }),
	);

	const jobHasFileErrors = $derived(jobFileDisplayList.some((i) => i.status === "error"));

	/** Tras un lote completado sin errores en archivos, persistir middleware preferido si el usuario no definió uno. */
	$effect(() => {
		if (!isTauriRuntime()) return;
		if (!jobSettled) {
			preferredLearnedForFinishedBatch = false;
			return;
		}
		if (activeJobItem?.status === "cancelled" || activeJobItem?.status === "error") return;
		if (jobHasFileErrors) return;
		if (preferredLearnedForFinishedBatch) return;
		preferredLearnedForFinishedBatch = true;
		void maybePersistPreferredModuleAfterSuccessfulBatch();
	});

	function isStepNavDisabled(stepNum: number): boolean {
		if (stepNum === wizardStep) return true;
		if (stepHistoryLocked) return true;
		if (stepNum < wizardStep) return false;
		if (stepNum === 2 && jobSettled) return false;
		return true;
	}

	function startNewSigningRound() {
		intentApplyGeneration++;
		wizardStep = 1;
		clearActiveBatchJobOnly();
		activeJobRef.current = null;
		progressPct = 0;
		progressSnapshot = null;
		batchFlowError = null;
		jobFileResults = [];
		pin = "";
		pinError = null;
		pinModalOpen = false;
		submitInFlight = false;
		stepHistoryLocked = false;
		clearPaths();
	}

	async function partitionPaths(list: string[]): Promise<string[]> {
		if (!isTauriRuntime()) return list;
		if (list.length === 0) return [];
		const { accepted, rejected } = await partitionBatchPdfPaths(list);
		if (rejected.length > 0) toastPdfRejections(rejected);
		return accepted;
	}

	async function refreshCerts(): Promise<number> {
		if (!isTauriRuntime()) return 0;
		try {
			certs = await pkcs11.listSigningCertificates();
			if (certs.length) {
				const saved = loadLastSigningCertId();
				if (saved && certs.some((c) => c.id_hex === saved)) {
					certId = saved;
				} else if (!certId.trim()) {
					certId = certs[0]?.id_hex ?? "";
				}
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
					: "Conexión reiniciada. Comprueba que la tarjeta esté bien puesta y pulsa «Actualizar».",
			);
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	async function refreshCertsWithBusy() {
		if (!isTauriRuntime()) return;
		busy = true;
		try {
			await refreshCerts();
		} finally {
			busy = false;
		}
	}

	async function tryListWithPin() {
		if (!isTauriRuntime() || !probePin.trim()) {
			toast.error("Introduce el PIN del DNIe.");
			return;
		}
		pinProbeBusy = true;
		try {
			certs = await pkcs11.pkcs11ListSigningWithPin(probePin.trim());
			if (certs.length && !certId) {
				certId = certs[0]?.id_hex ?? "";
			}
			slotsWithTokenCount = await pkcs11.pkcs11SlotCount().catch(() => slotsWithTokenCount);
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
		}
	}

	async function computeFirmadosDir(folderAbs: string): Promise<string> {
		const parent = await dirname(folderAbs);
		const base = await basename(folderAbs);
		return join(parent, `${base}_firmados`);
	}

	async function applyIngestedPaths(
		ingested: Awaited<ReturnType<typeof ingestDroppedPaths>>,
		merge: boolean,
	) {
		const accepted = await partitionPaths(ingested.pdfs);
		if (accepted.length === 0) return;

		if (merge && paths.length > 0 && ingested.sourceMode === "files") {
			paths = [...new Set([...paths, ...accepted])];
		} else {
			paths = accepted;
			sourceMode = ingested.sourceMode;
			folderPath = ingested.folderPath;
			outputDirForJob = ingested.outputDirForJob;
		}

		intentRequestId = null;
		intentDetachWizard();
	}

	async function handleDroppedPaths(rawPaths: string[]) {
		if (!isTauriRuntime() || rawPaths.length === 0) return;
		busy = true;
		try {
			const ingested = await ingestDroppedPaths(rawPaths, computeFirmadosDir);
			if (ingested.pdfs.length === 0) {
				toastInfo("No hay PDF en lo soltado.");
				return;
			}
			const beforeCount = paths.length;
			await applyIngestedPaths(ingested, beforeCount > 0);
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
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
				toastInfo("No hay PDFs en esa carpeta.");
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

	function jumpToStep(step: number) {
		if (stepHistoryLocked) return;
		if (step < 1 || step > TOTAL_STEPS) return;
		if (step === wizardStep) return;
		if (step > wizardStep && !(step === 2 && jobSettled)) return;
		wizardStep = step;
	}

	$effect(() => {
		if (certId.trim()) saveLastSigningCertId(certId);
	});

	function requestSign() {
		if (submitInFlight || busy) return;
		if (!certId.trim()) {
			toast.error("Selecciona un certificado.");
			return;
		}
		if (paths.length === 0) {
			toast.error("No hay PDFs.");
			return;
		}
		if (pinRequired && !pin.trim()) {
			pinError = null;
			pinModalOpen = true;
			return;
		}
		void submitBatch();
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
			pinError = "Introduce tu PIN.";
			pinModalOpen = true;
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
				stepHistoryLocked = false;
				pinModalOpen = true;
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
		batchFlowError = null;
		jobFileResults = [];
		progressPct = 0;
		progressSnapshot = null;
		setActiveBatchJobId(null);
		activeJobRef.current = null;
		saveSignaturePlacement({ col: sigGridCol, row: sigGridRow });
		pinModalOpen = false;
		wizardStep = 2;

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
			wizardStep = 1;
			stepHistoryLocked = false;
			if (pinRequired) pinModalOpen = true;
			upsertBatchQueueItem(pendingQueueId, { status: "error" });
			if (e instanceof LocalBackendInvokeError) {
				const detail = e.detail || e.code;
				if (detail.toLowerCase().includes("demasiado grande") || detail.includes("50 MiB")) {
					toastWarn("PDF demasiado grande", "Máximo 50 MiB por archivo.");
				} else if (
					detail.toLowerCase().includes("pin") ||
					detail.toLowerCase().includes("token") ||
					detail.toLowerCase().includes("sesión")
				) {
					pinError =
						"No hemos podido firmar con ese PIN. Revisa el PIN o vuelve a conectar el lector con la tarjeta dentro.";
					toastFail(pinError);
				} else {
					toastFail(detail);
				}
			} else if (e instanceof LocalApiHttpError) {
				const detail = extractJsonErrorMessage(e.body) ?? e.message;
				if (e.status === 400 && detail.includes("demasiado grande")) {
					toastWarn("PDF demasiado grande", "Máximo 50 MiB por archivo.");
				} else if (e.status === 401) {
					pinError =
						"No hemos podido firmar con ese PIN. Revisa el PIN o vuelve a conectar el lector con la tarjeta dentro.";
					toastFail(pinError);
				} else if (e.status === 403 && String(detail).toLowerCase().includes("origin")) {
					toastFail(
						"Origen no autorizado",
						"Añádelo en Ajustes → Orígenes permitidos.",
					);
				} else {
					toastFail(detail);
				}
			} else {
				toastFail(String(e));
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
		let unlistenDrop: (() => void) | undefined;

		void (async () => {
			if (isTauriRuntime()) {
				apiBase = await getLocalApiBaseUrl();
				await refreshCerts();
				unlistenDrop = await listenWindowFileDrop(
					(dropped) => void handleDroppedPaths(dropped),
					(over) => {
						dropHover = over;
					},
				);
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

		return () => {
			unlisten?.();
			unlistenDrop?.();
		};
	});

	$effect(() => {
		activeJobRef.current = batchQueue.activeBatchJobId;
	});

	$effect(() => {
		if (!jobSettled || activeJobItem?.status !== "error") {
			if (activeJobItem?.status !== "error") batchFlowError = null;
			return;
		}
		const jid = batchQueue.activeBatchJobId;
		if (!jid || jid.startsWith("pending-")) {
			batchFlowError = "No se pudo completar el lote.";
			return;
		}
		let cancelled = false;
		void (async () => {
			try {
				const snap = isTauriRuntime()
					? await ipcFetchBatchJobStatus(jid)
					: await fetchBatchJobStatus(jid, apiBase);
				if (cancelled) return;
				batchFlowError = snap.error?.trim() || "No se pudo completar el lote.";
			} catch {
				if (!cancelled) batchFlowError = "No se pudo completar el lote.";
			}
		})();
		return () => {
			cancelled = true;
		};
	});
</script>

<svelte:head>
	<title>Firmar — NexoSign</title>
</svelte:head>

<div class="mx-auto flex w-full max-w-lg min-h-0 flex-1 flex-col gap-2 overflow-hidden">
	<header class="flex shrink-0 items-center gap-2">
		<SignWizardStepper
			currentStep={wizardStep}
			isStepDisabled={isStepNavDisabled}
			onStepClick={jumpToStep}
			class="min-w-0 flex-1"
		/>
	</header>

	{#if !isTauriRuntime()}
		<Alert class="shrink-0 py-2">
			<AlertTitle class="text-sm">Requiere la app de escritorio</AlertTitle>
		</Alert>
	{/if}

	{#if wizardStep === 1}
		<section
			class="bg-card text-card-foreground relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border shadow-sm"
		>
			{#if dropHover && isTauriRuntime()}
				<div
					class="pointer-events-none absolute inset-0 z-30 flex flex-col items-center justify-center gap-3 rounded-xl bg-background/50 backdrop-blur-md transition-[background-color,backdrop-filter] duration-200 supports-backdrop-filter:backdrop-blur-md"
					aria-hidden="true"
				>
					<div
						class="border-primary/30 bg-card/85 flex flex-col items-center gap-2 rounded-2xl border px-8 py-6 shadow-lg backdrop-blur-sm"
					>
						<UploadIcon class="text-primary size-10" aria-hidden="true" />
						<p class="text-primary text-sm font-semibold">Suelta PDF o carpeta</p>
						<p class="text-muted-foreground text-xs">Se añadirán al lote</p>
					</div>
				</div>
			{/if}
			<div class="min-h-0 flex-1 overflow-y-auto p-4 sm:p-5 scrollbar-subtle">
				<SignComposePanel
					{paths}
					{sourceMode}
					{outputDirForJob}
					{busy}
					{dropHover}
					{certs}
					bind:certId
					{slotsWithTokenCount}
					listContextHint={listContextHint}
					{showPinProbe}
					bind:probePin
					{pinProbeBusy}
					{pinRequired}
					bind:pin
					bind:pinError
					bind:pinVisible
					bind:pinModalOpen
					bind:sigGridCol
					bind:sigGridRow
					{submitInFlight}
					signLabel={signButtonLabel}
					onBrowse={() => pickPdfs()}
					onBrowseFolder={() => pickFolder()}
					onClearPaths={() => clearPaths()}
					onRemoveAt={(i) => removeAt(i)}
					onRefreshCerts={() => refreshCertsWithBusy()}
					onResetReader={() => resetPkcs11ConnectionAndRefresh()}
					onTryListWithPin={() => tryListWithPin()}
					onSubmit={() => submitBatch()}
				/>
			</div>
			<div class="border-border/80 bg-muted/20 shrink-0 border-t px-4 py-3 sm:px-5">
				<Button
					type="button"
					size="lg"
					class="h-11 w-full gap-2 text-sm font-semibold"
					disabled={!canSign}
					onclick={() => requestSign()}
				>
					{#if submitInFlight}
						<Loader2Icon class="size-4 animate-spin" aria-hidden="true" />
						Firmando…
					{:else}
						<PenLineIcon class="size-4" aria-hidden="true" />
						{signButtonLabel}
					{/if}
				</Button>
			</div>
		</section>
	{/if}

	{#if wizardStep === 2}
		<section
			class="bg-card text-card-foreground flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border shadow-sm"
		>
			<div class="mx-auto flex min-h-0 w-full max-w-md flex-1 flex-col gap-3 overflow-hidden p-4 sm:p-5">
				<div class="flex shrink-0 items-center gap-3">
					{#if resultStepSigning}
						<div
							class="bg-primary/10 text-primary flex size-9 shrink-0 items-center justify-center rounded-full"
						>
							<Loader2Icon class="size-4 animate-spin" aria-hidden="true" />
						</div>
						<span class="text-sm font-semibold">Firmando</span>
						<Badge variant="secondary" class="h-5 text-[10px] tabular-nums">{progressPct}%</Badge>
					{:else if activeJobItem?.status === "cancelled"}
						<div
							class="bg-muted/50 text-muted-foreground flex size-9 shrink-0 items-center justify-center rounded-full"
						>
							<BanIcon class="size-4" aria-hidden="true" />
						</div>
						<span class="text-sm font-semibold">Cancelado</span>
					{:else if activeJobItem?.status === "error"}
						<div
							class="bg-destructive/15 text-destructive flex size-9 shrink-0 items-center justify-center rounded-full"
						>
							<TriangleAlertIcon class="size-4" aria-hidden="true" />
						</div>
						<span class="text-sm font-semibold">Error</span>
					{:else if jobSettled && jobHasFileErrors}
						<div
							class="flex size-9 shrink-0 items-center justify-center rounded-full bg-amber-500/15 text-amber-700 dark:text-amber-400"
						>
							<TriangleAlertIcon class="size-4" aria-hidden="true" />
						</div>
						<span class="text-sm font-semibold">Con avisos</span>
					{:else if jobSettled}
						<div
							class="flex size-9 shrink-0 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
						>
							<CircleCheckIcon class="size-4" aria-hidden="true" />
						</div>
						<span class="text-sm font-semibold">Listo</span>
						{#if progressSnapshot}
							<Badge variant="outline" class="h-5 text-[10px] tabular-nums">
								{progressSnapshot.actual}/{progressSnapshot.total}
							</Badge>
						{/if}
					{/if}
				</div>

				{#if activeJobItem?.status === "cancelled" && jobSettled}
					<Alert class="shrink-0 py-2">
						<AlertDescription class="text-xs">Lote detenido.</AlertDescription>
					</Alert>
				{/if}

				{#if batchFlowError && activeJobItem?.status === "error"}
					<Alert variant="destructive" class="shrink-0 py-2">
						<AlertDescription class="text-xs">{batchFlowError}</AlertDescription>
					</Alert>
				{/if}

				{#if resultStepSigning}
					<div class="shrink-0 space-y-1.5">
						{#if progressSubtitle}
							<p class="text-muted-foreground truncate text-xs" title={progressSubtitle}>{progressSubtitle}</p>
						{/if}
						<Progress value={progressPct} max={100} class="h-1.5 rounded-full" />
					</div>
				{/if}

				<div class="scrollbar-subtle min-h-0 flex-1 overflow-y-auto pb-1">
					<SignJobResults
						items={jobFileDisplayList}
						{outputDirForJob}
						{jobSettled}
						signing={resultStepSigning}
						activeFileIndex={progressSnapshot?.actual ?? null}
					/>
				</div>
			</div>
			<div
				class="border-border/80 bg-muted/20 flex shrink-0 items-center justify-end gap-2 border-t px-4 py-3 sm:px-5"
			>
				{#if canCancelBatchJobStep5}
					<Button type="button" variant="outline" size="sm" class="mr-auto" onclick={() => cancelJob()}>
						Cancelar
					</Button>
				{:else if batchQueue.activeBatchJobId && !jobSettled && activeJobItem?.status === "cancelling"}
					<Loader2Icon
						class="text-muted-foreground mr-auto size-4 animate-spin"
						aria-label="Cancelando"
					/>
				{/if}
				<Button
					type="button"
					size="lg"
					class="h-11 min-w-[9rem] gap-2 text-sm font-semibold"
					disabled={!jobSettled}
					onclick={() => startNewSigningRound()}
				>
					Nuevo lote
				</Button>
			</div>
		</section>
	{/if}
</div>
