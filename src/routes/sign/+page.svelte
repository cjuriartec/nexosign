<script lang="ts">
	import { onMount } from "svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import { basename, dirname, join } from "$lib/tauri/path";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Table from "$lib/components/ui/table/index.js";

	import SigningCertPicker from "$lib/components/signing-cert-picker.svelte";
	import SignConfirmPanel from "$lib/components/sign-confirm-panel.svelte";
	import SignJobResults from "$lib/components/sign-job-results.svelte";
	import SignWizardStepper from "$lib/components/sign-wizard-stepper.svelte";
	import SignWizardPanel from "$lib/components/sign-wizard-panel.svelte";
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
		wizardStep === 5 && !jobSettled && (submitInFlight || busy || batchQueue.activeBatchJobId !== null),
	);

	const selectedCert = $derived(certs.find((c) => c.id_hex === certId) ?? null);

	const pinRequired = $derived(pkcs11.pinRequiredInApp(selectedCert));

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
		batchFlowError = null;
		jobFileResults = [];
		pin = "";
		pinError = null;
		submitInFlight = false;
		stepHistoryLocked = false;
		clearPaths();
	}

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
		batchFlowError = null;
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

<div class="mx-auto flex min-h-full w-full max-w-lg flex-1 flex-col gap-2 pb-4">
	<header class="flex shrink-0 items-center gap-1.5 sm:gap-2">
		{#if wizardStep > 1}
			<Button
				type="button"
				variant="ghost"
				size="icon"
				class="size-8 shrink-0 sm:size-9"
				disabled={stepHistoryLocked}
				aria-label="Paso anterior"
				onclick={() => goBack()}
			>
				<ChevronLeftIcon class="size-4" />
			</Button>
		{/if}

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
		<SignWizardPanel>
			<div class="flex min-h-0 flex-1 flex-col gap-3">
				{#if paths.length === 0}
					<div class="grid min-h-0 flex-1 gap-3 sm:grid-cols-2">
						<button
							type="button"
							class="border-border/80 bg-muted/15 hover:border-primary/40 hover:bg-primary/5 flex min-h-[10rem] flex-1 flex-col items-center justify-center gap-3 rounded-xl border-2 border-dashed p-6 transition-colors disabled:opacity-50"
							disabled={busy}
							onclick={() => pickPdfs()}
						>
							<FileStackIcon class="text-muted-foreground size-11" aria-hidden="true" />
							<span class="text-sm font-medium">PDF</span>
						</button>
						<button
							type="button"
							class="border-border/80 bg-muted/15 hover:border-primary/40 hover:bg-primary/5 flex min-h-[10rem] flex-1 flex-col items-center justify-center gap-3 rounded-xl border-2 border-dashed p-6 transition-colors disabled:opacity-50"
							disabled={busy}
							onclick={() => pickFolder()}
						>
							<FolderOpenIcon class="text-muted-foreground size-11" aria-hidden="true" />
							<span class="text-sm font-medium">Carpeta</span>
						</button>
					</div>
				{:else}
					<div class="flex shrink-0 flex-wrap items-center gap-2">
						<Button
							type="button"
							size="sm"
							variant="secondary"
							class="gap-1.5"
							disabled={busy}
							onclick={() => pickPdfs()}
						>
							<FileStackIcon class="size-4" aria-hidden="true" />
							{sourceMode === "files" ? "Cambiar PDF" : "Elegir PDF"}
						</Button>
						<Button
							type="button"
							size="sm"
							variant="outline"
							class="gap-1.5"
							disabled={busy}
							onclick={() => pickFolder()}
						>
							<FolderOpenIcon class="size-4" aria-hidden="true" />
							{sourceMode === "folder" ? "Cambiar carpeta" : "Elegir carpeta"}
						</Button>
						<Button
							type="button"
							size="sm"
							variant="ghost"
							class="text-destructive ml-auto"
							disabled={busy}
							onclick={() => clearPaths()}
						>
							Limpiar
						</Button>
					</div>
					{#if outputDirForJob && sourceMode === "folder"}
						<p class="text-muted-foreground shrink-0 truncate font-mono text-[11px]" title={outputDirForJob}>
							{outputDirForJob}
						</p>
					{/if}
					<div class="border-border/70 min-h-0 flex-1 overflow-hidden rounded-lg border">
						<Table.Root>
							<Table.Header class="bg-muted/30 sticky top-0 z-10">
								<Table.Row>
									<Table.Head class="w-8 py-2 text-xs">#</Table.Head>
									<Table.Head class="py-2 text-xs">{paths.length} PDF</Table.Head>
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
					</div>
				{/if}
			</div>
			{#snippet footer()}
				<Button
					type="button"
					size="sm"
					class="gap-1"
					disabled={busy || paths.length === 0}
					onclick={() => step1Continue()}
				>
					Siguiente
					<ChevronRightIcon class="size-4" aria-hidden="true" />
				</Button>
			{/snippet}
		</SignWizardPanel>
	{/if}

	{#if wizardStep === 2}
		<SignWizardPanel>
			<div class="flex min-h-0 flex-1 flex-col items-center justify-center gap-5 py-2">
				<Badge variant="secondary" class="h-6 px-2.5 font-mono text-xs tabular-nums">
					{sigGridCol + 1} · {sigGridRow + 1}
				</Badge>
				<div
					class="overflow-hidden rounded-xl border border-border bg-linear-to-b from-muted/30 to-muted/10 p-3 shadow-inner"
				>
					{#each [0, 1, 2, 3, 4] as row}
						<div class="flex gap-1.5 pb-1.5 last:pb-0">
							{#each [0, 1, 2] as col}
								<button
									type="button"
									class={cn(
										"flex size-10 shrink-0 items-center justify-center rounded-lg border text-xs font-semibold transition-all sm:size-11",
										sigGridCol === col && sigGridRow === row
											? "border-primary bg-primary text-primary-foreground shadow-md"
											: "border-border/70 bg-background text-muted-foreground hover:border-primary/30 hover:bg-muted/60",
									)}
									aria-label="Casilla {row * SIG_GRID_COLS + col + 1}"
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
			</div>
			{#snippet footer()}
				<Button type="button" size="sm" class="gap-1" onclick={() => step2PlacementContinue()}>
					Siguiente
					<ChevronRightIcon class="size-4" aria-hidden="true" />
				</Button>
			{/snippet}
		</SignWizardPanel>
	{/if}

	{#if wizardStep === 3}
		<SignWizardPanel>
			<SigningCertPicker
				{certs}
				bind:certId
				{busy}
				slotsWithToken={slotsWithTokenCount}
				helpVariant="brief"
				showDedupeNote={false}
				compact
				class="min-h-0 flex-1"
				onRefresh={() => refreshCertsWithBusy()}
				onResetReader={() => resetPkcs11ConnectionAndRefresh()}
			/>
			{#snippet footer()}
				<Button
					type="button"
					size="sm"
					class="gap-1"
					disabled={busy || !certId.trim() || certs.length === 0}
					onclick={() => step3CertContinue()}
				>
					Siguiente
					<ChevronRightIcon class="size-4" aria-hidden="true" />
				</Button>
			{/snippet}
		</SignWizardPanel>
	{/if}

	{#if wizardStep === 4}
		<SignWizardPanel contentCentered>
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
		</SignWizardPanel>
	{/if}

	{#if wizardStep === 5}
		<SignWizardPanel class="w-full">
			<div class="mx-auto flex min-h-0 w-full max-w-md flex-1 flex-col gap-3 overflow-hidden">
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

				<div class="min-h-0 flex-1 overflow-y-auto">
					<SignJobResults
						items={jobFileDisplayList}
						{outputDirForJob}
						{jobSettled}
						signing={resultStepSigning}
						activeFileIndex={progressSnapshot?.actual ?? null}
					/>
				</div>
			</div>
			{#snippet footer()}
				{#if canCancelBatchJobStep5}
					<Button type="button" variant="outline" size="sm" onclick={() => cancelJob()}>Cancelar</Button>
				{:else if batchQueue.activeBatchJobId && !jobSettled && activeJobItem?.status === "cancelling"}
					<Loader2Icon class="text-muted-foreground mr-auto size-4 animate-spin" aria-label="Cancelando" />
				{/if}
				<Button type="button" size="sm" disabled={!jobSettled} onclick={() => startNewSigningRound()}>
					Nuevo lote
				</Button>
			{/snippet}
		</SignWizardPanel>
	{/if}
</div>
