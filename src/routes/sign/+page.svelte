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
	import { Badge } from "$lib/components/ui/badge/index.js";
	import { page } from "$app/state";
	import { postBatchSign, type BatchSignBody, LocalApiHttpError, extractJsonErrorMessage } from "$lib/api/local-api";
	import { subscribeProgress, type ProgressPayload } from "$lib/events/progress";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import { partitionBatchPdfPaths } from "$lib/tauri/batch-validation";
	import { enumeratePdfsUnderFolder } from "$lib/tauri/batch";
	import { getBatchSignIntent } from "$lib/tauri/batch-sign-intent";
	import { isPkcs11NoTokenError } from "$lib/tauri/pkcs11-errors";
	import { cancelBatchJob, getLocalApiBaseUrl } from "$lib/tauri/settings";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { getHumanNameFromDn, extractDniFromDn } from "$lib/signature-appearance";
	import { renderSignatureSealPngBase64 } from "$lib/signature-appearance-render";
	import EyeIcon from "@lucide/svelte/icons/eye";
	import EyeOffIcon from "@lucide/svelte/icons/eye-off";
	import Loader2Icon from "@lucide/svelte/icons/loader-2";
	import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import FileStackIcon from "@lucide/svelte/icons/files";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import CheckIcon from "@lucide/svelte/icons/check";
	import { cn } from "$lib/utils.js";

	const SIGN_STEPS = [
		{ step: 1, title: "Archivos", hint: "PDF sueltos o carpeta entera" },
		{ step: 2, title: "Ubicación", hint: "Casilla en la 1.ª página (pie discreto)" },
		{ step: 3, title: "Certificado", hint: "Tu identidad para firmar" },
		{ step: 4, title: "Confirmar", hint: "PIN y firma (no se guarda el PIN)" },
	] as const;

	/** Rejilla 3×5: 3 columnas (ancho) × 5 filas (alto); col 0 izquierda, fila 0 cabecera del PDF. */
	const SIG_GRID_COLS = 3;
	const SIG_GRID_ROWS = 5;

	let paths = $state<string[]>([]);
	/** Origen del lote actual: archivos sueltos vs carpeta (salida agrupada). */
	let sourceMode = $state<"files" | "folder" | null>(null);
	let folderPath = $state<string | null>(null);
	/** Directorio absoluto `{padre}/{nombre}_firmados` cuando sourceMode === folder */
	let outputDirForJob = $state<string | null>(null);

	let certs = $state<SigningCertSummary[]>([]);
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
	/** Evita ejecutar dos veces la misma `?intent=` si `replaceState` no actualiza al instante el store de rutas. */
	let handledIntentQuery = $state<string | null>(null);

	let activeJobId = $state<string | null>(null);
	const activeJobRef: { current: string | null } = { current: null };
	let progressPct = $state(0);
	let logLines = $state<string[]>([]);
	/** Último tick de progreso (para título y subtítulo del panel). */
	let progressSnapshot = $state<{
		actual: number;
		total: number;
		fileLabel: string;
	} | null>(null);

	let logViewportEl = $state<HTMLElement | null>(null);

	const showProgressPanel = $derived(
		activeJobId !== null || logLines.length > 0 || progressPct > 0,
	);

	const progressSubtitle = $derived.by(() => {
		if (!progressSnapshot) {
			return activeJobId ? "Preparando firma…" : "";
		}
		const { actual, total, fileLabel } = progressSnapshot;
		const base = `Documento ${actual} de ${total}`;
		return fileLabel ? `${base} · ${fileLabel}` : base;
	});

	const executionFinished = $derived(activeJobId !== null && progressPct >= 100);

	const wizardBarPct = $derived(Math.round((wizardStep / 4) * 100));

	function pushLog(line: string) {
		logLines = [...logLines, line].slice(-120);
	}

	function logLineTone(line: string): "muted" | "ok" | "err" {
		const lower = line.toLowerCase();
		if (
			/\berror\b/.test(lower) ||
			/\bfallo\b/.test(lower) ||
			/\bfalló\b/.test(lower) ||
			/\bincorrecto\b/.test(lower) ||
			line.includes(" · ") &&
				line.split(" · ").length >= 3
		) {
			return "err";
		}
		if (/\bok\b|\bcompletad|\bfirmad/i.test(lower)) {
			return "ok";
		}
		return "muted";
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
		const payload = await getBatchSignIntent(intentParam);
		if (!payload) {
			toast.error("La solicitud no existe o caducó (30 min). Abre el enlace desde la integración de nuevo.");
			return;
		}
		paths = await partitionPaths([...payload.inputs]);
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
			toast.error("Indica el PIN del token para firmar.");
			pinError = "Introduce el PIN del token.";
			return;
		}

		if (isTauriRuntime()) {
			try {
				await pkcs11.pkcs11VerifyPin(pin.trim(), certId.trim());
				toast.success("PIN correcto");
			} catch (e) {
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
		activeJobId = null;
		activeJobRef.current = null;
		try {
			const body: BatchSignBody = {
				cert_id_hex: certId.trim(),
				inputs: paths,
				pin: pin.trim(),
				signature_grid: { col: sigGridCol, row: sigGridRow },
			};
			const selectedCert = certs.find((c) => c.id_hex === certId.trim()) ?? null;
			try {
				const seal = await renderSignatureSealPngBase64(selectedCert);
				if (seal) body.signature_seal_png_base64 = seal;
			} catch {
				/* la API usará la apariencia vectorial de respaldo */
			}
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
				Paso <span class="text-foreground font-medium">{wizardStep}</span> de 4 · sigue el orden o vuelve atrás con el botón o tocando un paso ya hecho.
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
		<div class="grid grid-cols-4 gap-1.5 sm:gap-2">
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
				<Card.Description class="text-xs">
					{SIGN_STEPS[1].hint}. La firma usa tu certificado y el sello de Certificados; aquí solo eliges la casilla visible en la primera página.
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<p class="text-muted-foreground text-xs leading-snug">
					Hoja en vertical: 5 columnas × 7 filas. Fila superior = cabecera del PDF. Cada PDF del lote usa la misma casilla.
				</p>
				<div class="mx-auto w-fit overflow-hidden rounded-lg border border-border bg-muted/25 shadow-sm">
					{#each [0, 1, 2, 3, 4] as row}
						<div class="flex border-b border-border/70 last:border-b-0">
							{#each [0, 1, 2] as col}
								<button
									type="button"
									class={cn(
										"flex h-9 w-9 shrink-0 items-center justify-center border-r border-border/70 text-[10px] font-medium transition-colors last:border-r-0 sm:h-10 sm:w-10",
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
				<p class="text-muted-foreground text-xs">
					Selección: columna {sigGridCol + 1} · fila {sigGridRow + 1}
				</p>
				<Button
					type="button"
					onclick={() => step2PlacementContinue()}
					aria-label={`Siguiente paso: ${SIGN_STEPS[2].title}`}
				>
					Continuar
					<span class="text-primary-foreground/85 ml-1 hidden font-normal sm:inline">
						→ {SIGN_STEPS[2].title}
					</span>
				</Button>
			</Card.Content>
		</Card.Root>
	{/if}

	{#if wizardStep === 3}
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">{SIGN_STEPS[2].title}</Card.Title>
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
					<div class="flex flex-wrap items-center gap-2">
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
							onclick={() => step3CertContinue()}
							aria-label={`Siguiente paso: ${SIGN_STEPS[3].title}`}
						>
							Continuar
							<span class="text-primary-foreground/85 ml-1 hidden font-normal sm:inline">
								→ {SIGN_STEPS[3].title}
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
								Solo si el lector falla o hubo problemas con el PIN…
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

	{#if wizardStep === 4}
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">{SIGN_STEPS[3].title}</Card.Title>
				<Card.Description class="text-xs">
					Introduce el PIN del token y pulsa <span class="text-foreground font-medium">Firmar</span> (o Enter). El PIN no se guarda.
				</Card.Description>
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
						<span class="text-muted-foreground"> (rejilla 3×5)</span>
					</p>
					{#if outputDirForJob}
						<p class="truncate font-mono" title={outputDirForJob}>{outputDirForJob}</p>
					{:else}
						<p><code class="bg-muted rounded px-1">*_firmado.pdf</code> junto a cada PDF</p>
					{/if}
				</div>
				<div class="grid max-w-md gap-2">
					<Label for="pin-confirm">PIN del token</Label>
					<div class="relative">
						<Input
							id="pin-confirm"
							type={pinVisible ? "text" : "password"}
							autocomplete="off"
							bind:value={pin}
							placeholder="PIN"
							class={cn(
								"pr-10",
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
							class="text-muted-foreground absolute right-1 top-1/2 h-8 w-8 -translate-y-1/2"
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
						<p class="text-sm font-medium text-destructive">{pinError}</p>
					{/if}
				</div>
				<div class="flex flex-wrap gap-2">
					<Button
						type="button"
						disabled={busy || paths.length === 0 || !certId.trim() || !pin.trim()}
						onclick={() => submitBatch()}
					>
						Firmar
					</Button>
					<Button type="button" variant="outline" disabled={busy} onclick={() => cancelJob()}>Cancelar cola</Button>
				</div>
			</Card.Content>
		</Card.Root>
	{/if}
	{#if showProgressPanel}
		<Card.Root
			class={cn(
				"border-primary/15 shadow-sm transition-shadow",
				activeJobId && !executionFinished && "ring-primary/10 ring-2",
			)}
			size="sm"
		>
			<Card.Header class="border-border/60 gap-3 border-b pb-4">
				<div class="flex flex-wrap items-start justify-between gap-3">
					<div class="space-y-1">
						<div class="flex flex-wrap items-center gap-2">
							<Card.Title class="text-base tracking-tight">Ejecución del lote</Card.Title>
							{#if activeJobId && !executionFinished}
								<Badge variant="secondary" class="gap-1 font-normal">
									<Loader2Icon class="size-3 animate-spin" aria-hidden="true" />
									En curso
								</Badge>
							{:else if executionFinished}
								<Badge variant="outline" class="gap-1 border-emerald-500/40 font-normal text-emerald-700 dark:text-emerald-400">
									<CircleCheckIcon class="size-3" aria-hidden="true" />
									Listo
								</Badge>
							{/if}
						</div>
						<Card.Description class="text-xs leading-snug">
							{#if progressSubtitle}
								{progressSubtitle}
							{:else}
								Esperando el primer evento del firmador…
							{/if}
						</Card.Description>
					</div>
					{#if activeJobId}
						<span
							class="bg-muted/80 text-muted-foreground max-w-[min(100%,14rem)] truncate rounded-md px-2 py-1 font-mono text-[10px] leading-none"
							title={activeJobId}
						>
							Job {activeJobId.slice(0, 8)}…
						</span>
					{/if}
				</div>
				<div class="flex flex-wrap items-end justify-between gap-4 pt-1">
					<div class="flex items-baseline gap-1.5">
						<span class="text-foreground tabular-nums text-4xl font-semibold tracking-tight">{progressPct}</span>
						<span class="text-muted-foreground pb-1 text-sm font-medium">%</span>
					</div>
					{#if progressSnapshot}
						<p class="text-muted-foreground text-xs tabular-nums">
							{progressSnapshot.actual} / {progressSnapshot.total} PDF
						</p>
					{/if}
				</div>
				<Progress value={progressPct} max={100} class="h-2.5 rounded-full" />
			</Card.Header>
			<Card.Content class="space-y-3 pt-2">
				<div class="flex items-center justify-between gap-2">
					<p class="text-muted-foreground text-xs font-medium uppercase tracking-wider">Registro</p>
					{#if logLines.length > 0}
						<span class="text-muted-foreground font-mono text-[10px] tabular-nums">{logLines.length} línea(s)</span>
					{/if}
				</div>
				<ScrollArea.Root
					bind:viewportRef={logViewportEl}
					class="bg-muted/25 dark:bg-muted/15 h-52 rounded-lg border shadow-inner"
				>
					<div class="space-y-0 p-2 font-mono text-[11px] leading-relaxed">
						{#if logLines.length === 0}
							<p class="text-muted-foreground px-2 py-6 text-center text-xs">
								Cuando la firma avance, aquí verás cada paso (archivo actual, avisos y errores).
							</p>
						{:else}
							{#each logLines as line, i}
								{@const tone = logLineTone(line)}
								<div
									class={cn(
										"border-border/50 rounded-md border-l-2 py-1.5 pl-2 pr-1 transition-colors",
										i === logLines.length - 1 ? "border-primary bg-primary/6" : "border-transparent",
										tone === "err" && "text-destructive",
										tone === "ok" && "text-emerald-700 dark:text-emerald-400",
										tone === "muted" && "text-foreground/85",
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
