<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import * as Table from "$lib/components/ui/table/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import ThemeToggle from "$lib/components/theme-toggle.svelte";
	import {
		addAllowedOrigin,
		listAllowedOrigins,
		removeAllowedOrigin,
		getLocalApiBaseUrl,
		clearLocalApiTempCache,
		getBatchJobMaxSecsConfig,
		setBatchJobMaxSecs,
		listPkcs11DriverPaths,
		addPkcs11DriverPath,
		removePkcs11DriverPath,
		setPkcs11DriverPathsOrder,
		resetPkcs11DriverPathsToDefaults,
		getPkcs11PreferredModule,
		setPkcs11PreferredModule,
		listPkcs11EffectiveModulePaths,
	} from "$lib/tauri/settings";
	import { cn } from "$lib/utils.js";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { ask } from "@tauri-apps/plugin-dialog";
	import { LOCAL_API_BASE } from "$lib/config/constants";
	import { fetchHealth, fetchPing } from "$lib/api/local-api";
	import { ipcFetchHealth, ipcFetchPing } from "$lib/tauri/local-backend";
	import * as ScrollArea from "$lib/components/ui/scroll-area/index.js";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { Pkcs11Diagnostics, Pkcs11ProbeCertificateListing } from "$lib/tauri/pkcs11";
	import CpuIcon from "@lucide/svelte/icons/cpu";
	import GripVerticalIcon from "@lucide/svelte/icons/grip-vertical";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";

	let origins = $state<string[]>([]);
	let newOrigin = $state("");
	let apiUrl = $state(LOCAL_API_BASE);
	let loading = $state(false);
	let diagLoading = $state(false);
	let health = $state<{ status: string; service: string; version: string } | null>(null);
	let pingOk = $state<boolean | null>(null);

	let pkcsDiag = $state<Pkcs11Diagnostics | null>(null);
	let pkcsCertProbe = $state<Pkcs11ProbeCertificateListing | null>(null);
	let pkcsBusy = $state(false);
	let pkcs11Paths = $state<string[]>([]);
	let newDriverPath = $state("");
	let pathsBusy = $state(false);
	let effectiveModulePaths = $state<string[]>([]);
	/** `__auto__` = sin preferencia (orden de tabla + incorporadas). */
	let preferredChoice = $state("__auto__");
	/** Con «Automático»: biblioteca PKCS#11 que la app acaba de cargar tras resolver prioridades. */
	let autoResolvedModulePath = $state<string | null>(null);
	type PathDragState = {
		fromIndex: number;
		pointerId: number;
		armed: boolean;
		startX: number;
		startY: number;
		label: string;
	};
	let pathDrag = $state<PathDragState | null>(null);
	let pathDropIndex = $state<number | null>(null);
	let pathGhostX = $state(0);
	let pathGhostY = $state(0);
	let pathListEl = $state<HTMLElement | null>(null);
	let cacheClearing = $state(false);
	const BATCH_JOB_TIMEOUT_MIN = 60;
	const BATCH_JOB_TIMEOUT_MAX = 604800;
	let batchJobTimeoutStored = $state(300);
	let batchJobTimeoutEffective = $state(300);
	let batchJobTimeoutLockedByEnv = $state(false);
	let batchJobTimeoutBusy = $state(false);
	const PATH_DRAG_THRESHOLD = 4;

	async function probeActiveModulePath() {
		if (!isTauriRuntime()) return;
		try {
			autoResolvedModulePath = await pkcs11.probePkcs11ModulePath();
		} catch {
			autoResolvedModulePath = null;
		}
	}

	async function syncPkcsLists() {
		if (!isTauriRuntime()) return;
		pkcs11Paths = await listPkcs11DriverPaths();
		effectiveModulePaths = await listPkcs11EffectiveModulePaths();
		const p = await getPkcs11PreferredModule();
		preferredChoice = p ?? "__auto__";
		await probeActiveModulePath();
	}

	async function refreshPkcs11Listing() {
		if (!isTauriRuntime()) return;
		pkcsBusy = true;
		try {
			await syncPkcsLists();
			toast.message("Listado actualizado");
		} catch (e) {
			toast.error(String(e));
		} finally {
			pkcsBusy = false;
		}
	}

	async function diagnosePkcs11Slots() {
		if (!isTauriRuntime()) return;
		pkcsBusy = true;
		try {
			pkcsDiag = await pkcs11.pkcs11DiagnoseSlots();
			pkcsCertProbe = await pkcs11.pkcs11ProbeCertificateListing();
			toast.success("Diagnóstico PKCS#11 listo");
		} catch (e) {
			toast.error(String(e));
		} finally {
			pkcsBusy = false;
		}
	}

	async function probeChipCertificates() {
		if (!isTauriRuntime()) return;
		pkcsBusy = true;
		try {
			pkcsCertProbe = await pkcs11.pkcs11ProbeCertificateListing();
			toast.success("Exploración del chip completada");
		} catch (e) {
			toast.error(String(e));
		} finally {
			pkcsBusy = false;
		}
	}

	async function persistPreferredChoice(v: string | undefined) {
		if (!isTauriRuntime()) return;
		const next = v ?? "__auto__";
		pathsBusy = true;
		try {
			await setPkcs11PreferredModule(next === "__auto__" ? null : next);
			preferredChoice = next;
			await probeActiveModulePath();
		} catch (e) {
			toast.error(String(e));
		} finally {
			pathsBusy = false;
		}
	}

	async function addDriverPathRow() {
		const p = newDriverPath.trim();
		if (!p || !isTauriRuntime()) return;
		pathsBusy = true;
		try {
			await addPkcs11DriverPath(p);
			newDriverPath = "";
			toast.success("Ruta añadida");
			await syncPkcsLists();
		} catch (e) {
			toast.error(String(e));
		} finally {
			pathsBusy = false;
		}
	}

	async function removeDriverPathRow(path: string) {
		if (!isTauriRuntime()) return;
		pathsBusy = true;
		try {
			await removePkcs11DriverPath(path);
			toast.message("Ruta eliminada");
			await syncPkcsLists();
		} catch (e) {
			toast.error(String(e));
		} finally {
			pathsBusy = false;
		}
	}

	function startPathDrag(e: PointerEvent, index: number) {
		if (e.button !== 0 || pathsBusy) return;
		const label = pkcs11Paths[index];
		if (!label) return;
		e.preventDefault();
		pathDrag = {
			fromIndex: index,
			pointerId: e.pointerId,
			armed: false,
			startX: e.clientX,
			startY: e.clientY,
			label,
		};
		pathGhostX = e.clientX;
		pathGhostY = e.clientY;
		window.addEventListener("pointermove", onPathPointerMove, { passive: false });
		window.addEventListener("pointerup", onPathPointerUp);
		window.addEventListener("pointercancel", onPathPointerCancel);
		window.addEventListener("keydown", onPathKeyDown);
	}

	function onPathPointerMove(e: PointerEvent) {
		if (!pathDrag) return;
		if (e.pointerId !== pathDrag.pointerId) return;
		if (!pathDrag.armed) {
			const dx = e.clientX - pathDrag.startX;
			const dy = e.clientY - pathDrag.startY;
			if (dx * dx + dy * dy < PATH_DRAG_THRESHOLD * PATH_DRAG_THRESHOLD) return;
			pathDrag = { ...pathDrag, armed: true };
			document.body.classList.add("nexo-dnd-active");
		}
		e.preventDefault();
		pathGhostX = e.clientX;
		pathGhostY = e.clientY;
		pathDropIndex = computePathDropAt(e.clientX, e.clientY);
	}

	function onPathPointerUp(e: PointerEvent) {
		if (!pathDrag) return;
		if (e.pointerId !== pathDrag.pointerId) return;
		const armed = pathDrag.armed;
		const fromIndex = pathDrag.fromIndex;
		const target = pathDropIndex;
		finishPathDrag();
		if (!armed) return;
		if (target === null) return;
		let toIndex = target;
		if (toIndex > fromIndex) toIndex--;
		if (toIndex === fromIndex) return;
		void reorderDriverPaths(fromIndex, toIndex);
	}

	function onPathPointerCancel() {
		finishPathDrag();
	}

	function onPathKeyDown(e: KeyboardEvent) {
		if (e.key === "Escape") finishPathDrag();
	}

	function finishPathDrag() {
		pathDrag = null;
		pathDropIndex = null;
		document.body.classList.remove("nexo-dnd-active");
		window.removeEventListener("pointermove", onPathPointerMove);
		window.removeEventListener("pointerup", onPathPointerUp);
		window.removeEventListener("pointercancel", onPathPointerCancel);
		window.removeEventListener("keydown", onPathKeyDown);
	}

	function computePathDropAt(x: number, y: number): number | null {
		if (!pathListEl) return null;
		const rect = pathListEl.getBoundingClientRect();
		const SLACK = 80;
		if (
			x < rect.left - SLACK ||
			x > rect.right + SLACK ||
			y < rect.top - SLACK ||
			y > rect.bottom + SLACK
		) {
			return null;
		}
		const rows = Array.from(pathListEl.querySelectorAll<HTMLElement>("[data-path-row]"));
		if (rows.length === 0) return 0;
		for (let i = 0; i < rows.length; i++) {
			const r = rows[i].getBoundingClientRect();
			if (y < r.top + r.height / 2) return i;
		}
		return rows.length;
	}

	async function reorderDriverPaths(fromIndex: number, toIndex: number) {
		if (
			fromIndex === toIndex ||
			fromIndex < 0 ||
			toIndex < 0 ||
			fromIndex >= pkcs11Paths.length ||
			toIndex >= pkcs11Paths.length
		)
			return;
		pathsBusy = true;
		try {
			const arr = [...pkcs11Paths];
			const [item] = arr.splice(fromIndex, 1);
			arr.splice(toIndex, 0, item);
			await setPkcs11DriverPathsOrder(arr);
			pkcs11Paths = arr;
			await syncPkcsLists();
		} catch (e) {
			toast.error(String(e));
		} finally {
			pathsBusy = false;
		}
	}

	async function resetDriverPathsDefaults() {
		if (!isTauriRuntime()) return;
		pathsBusy = true;
		try {
			await resetPkcs11DriverPathsToDefaults();
			toast.success("Lista restaurada a las rutas incorporadas");
			await syncPkcsLists();
		} catch (e) {
			toast.error(String(e));
		} finally {
			pathsBusy = false;
		}
	}

	async function syncBatchJobTimeout() {
		if (!isTauriRuntime()) return;
		try {
			const c = await getBatchJobMaxSecsConfig();
			batchJobTimeoutStored = c.storedSecs;
			batchJobTimeoutEffective = c.effectiveSecs;
			batchJobTimeoutLockedByEnv = c.lockedByEnv;
		} catch (e) {
			toast.error(String(e));
		}
	}

	async function saveBatchJobTimeout() {
		if (!isTauriRuntime()) return;
		let n = Math.round(Number(batchJobTimeoutStored));
		if (!Number.isFinite(n)) {
			toast.error("Introduce un número válido");
			return;
		}
		n = Math.min(BATCH_JOB_TIMEOUT_MAX, Math.max(BATCH_JOB_TIMEOUT_MIN, n));
		batchJobTimeoutBusy = true;
		try {
			await setBatchJobMaxSecs(n);
			toast.success("Tiempo máximo de cola guardado");
			await syncBatchJobTimeout();
		} catch (e) {
			toast.error(String(e));
		} finally {
			batchJobTimeoutBusy = false;
		}
	}

	async function refresh() {
		if (!isTauriRuntime()) return;
		loading = true;
		try {
			origins = await listAllowedOrigins();
			apiUrl = await getLocalApiBaseUrl();
		} catch (e) {
			toast.error(String(e));
		} finally {
			loading = false;
		}
	}

	async function confirmClearApiCache() {
		if (!isTauriRuntime()) return;
		const ok = await ask(
			"Se eliminarán las carpetas temporales nexosign-intent-uploads y nexosign-batch-signed (subidas del portal y PDF listos para descarga por HTTP). Las rutas en memoria del servicio local se vacían para que no queden enlaces rotos. ¿Continuar?",
			{
				title: "Limpiar caché del servicio local",
				kind: "warning",
			},
		);
		if (!ok) return;
		cacheClearing = true;
		try {
			const r = await clearLocalApiTempCache();
			const parts: string[] = [];
			if (r.intentUploadsRemoved) parts.push("subidas intent");
			if (r.batchSignedRemoved) parts.push("PDF firmados temporales");
			if (r.signedJobPathsCleared) parts.push("mapa de descargas");
			toast.success(
				parts.length > 0 ? `Limpiado: ${parts.join(", ")}` : "No había carpetas que borrar",
			);
		} catch (e) {
			toast.error(String(e));
		} finally {
			cacheClearing = false;
		}
	}

	async function refreshDiagnostics() {
		diagLoading = true;
		try {
			if (isTauriRuntime()) {
				apiUrl = await getLocalApiBaseUrl();
				health = await ipcFetchHealth();
				const p = await ipcFetchPing();
				pingOk = p.ok;
			} else {
				apiUrl = LOCAL_API_BASE;
				health = await fetchHealth(LOCAL_API_BASE);
				const p = await fetchPing(LOCAL_API_BASE);
				pingOk = p.ok;
			}
		} catch {
			health = null;
			pingOk = false;
		} finally {
			diagLoading = false;
		}
	}

	async function add() {
		const o = newOrigin.trim();
		if (!o) return;
		try {
			await addAllowedOrigin(o);
			newOrigin = "";
			toast.success("Origen añadido");
			await refresh();
		} catch (e) {
			toast.error(String(e));
		}
	}

	async function remove(o: string) {
		try {
			await removeAllowedOrigin(o);
			toast.message("Origen eliminado");
			await refresh();
		} catch (e) {
			toast.error(String(e));
		}
	}

	onMount(() => {
		void refreshDiagnostics();
		if (isTauriRuntime()) {
			void refresh();
			void syncBatchJobTimeout();
			void (async () => {
				try {
					await syncPkcsLists();
				} catch (e) {
					toast.error(String(e));
				}
			})();
		}
	});
</script>

<svelte:head>
	<title>Ajustes — NexoSign</title>
</svelte:head>

<div class="mx-auto max-w-3xl space-y-8">
	<div>
		<h1 class="text-3xl font-semibold tracking-tight">Ajustes</h1>
		<p class="text-muted-foreground mt-1 text-sm">
			Tema, tiempo máximo de cola, lector de tarjetas, sitios permitidos, caché del servicio local y comprobación del servicio.
		</p>
	</div>

	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">Apariencia</Card.Title>
			<Card.Description>
				Aplica en toda la aplicación. «Sistema» sigue el modo claro u oscuro del dispositivo.
			</Card.Description>
		</Card.Header>
		<Card.Content class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
			<ThemeToggle />
		</Card.Content>
	</Card.Root>

	{#if isTauriRuntime()}
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Cola de firma</Card.Title>
				<Card.Description>
					Ventana máxima para intents del portal y trabajos batch encolados en SQLite. Si existe la variable
					<code class="font-mono text-[11px]">NEXOSIGN_BATCH_JOB_MAX_SECS</code>, tiene prioridad sobre este valor (reinicia tras cambiar el entorno).
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<p class="text-muted-foreground text-xs">
					Vigente ahora:
					<span class="text-foreground font-mono font-medium">{batchJobTimeoutEffective}</span> s
				</p>
				{#if batchJobTimeoutLockedByEnv}
					<p class="text-sm text-amber-800 dark:text-amber-200">
						Edición desactivada: el proceso tiene definida
						<span class="font-mono">NEXOSIGN_BATCH_JOB_MAX_SECS</span>. Elimínala y reinicia la app para usar
						este ajuste.
					</p>
				{:else}
					<div class="flex flex-col gap-3 sm:flex-row sm:items-end">
						<div class="grid min-w-0 flex-1 gap-2">
							<Label for="batch-job-timeout">
								Segundos ({BATCH_JOB_TIMEOUT_MIN}–{BATCH_JOB_TIMEOUT_MAX})
							</Label>
							<Input
								id="batch-job-timeout"
								type="number"
								min={BATCH_JOB_TIMEOUT_MIN}
								max={BATCH_JOB_TIMEOUT_MAX}
								step="1"
								bind:value={batchJobTimeoutStored}
								disabled={batchJobTimeoutBusy}
								class="font-mono"
							/>
						</div>
						<Button
							type="button"
							variant="secondary"
							class="h-9 shrink-0"
							disabled={batchJobTimeoutBusy}
							onclick={() => void saveBatchJobTimeout()}>Guardar</Button>
					</div>
				{/if}
			</Card.Content>
		</Card.Root>
	{/if}

	{#if isTauriRuntime()}
		<Card.Root>
			<Card.Header class="flex flex-row items-start justify-between space-y-0">
				<div>
					<Card.Title class="text-base">Lector de DNIe y tarjetas</Card.Title>
					<Card.Description>
						Aquí eliges el <strong>controlador del lector</strong> (PKCS#11) para DNIe o tarjeta. Si no estás
						seguro, deja «Automático». En <strong>Windows</strong>, al listar certificados para firmar también
						pueden aparecer los del almacén <strong>Personal</strong> (MY) con clave RSA CNG; eso no se
						configura en esta tabla. Los <code class="bg-muted rounded px-1 font-mono text-[11px]">.pfx</code>
						solo en disco siguen sin listarse salvo que uses un middleware PKCS#11 o los instales en MY.
					</Card.Description>
				</div>
				<CpuIcon class="text-muted-foreground size-5" />
			</Card.Header>
			<Card.Content class="space-y-6">
				<div class="flex flex-col gap-3 sm:flex-row sm:items-end">
					<div class="grid min-w-0 flex-1 gap-2">
						<Label for="p11-preferred">Controlador del lector</Label>
						<Select.Root
							type="single"
							bind:value={preferredChoice}
							onValueChange={(v) => void persistPreferredChoice(v)}
						>
							<Select.Trigger
								id="p11-preferred"
								class="w-full max-w-xl justify-between font-normal"
								disabled={pathsBusy || pkcsBusy}
							>
								{#if preferredChoice === "__auto__"}
									Automático
								{:else}
									<span class="block max-w-[min(100%,28rem)] truncate text-left font-mono text-xs">{preferredChoice}</span>
								{/if}
							</Select.Trigger>
							<Select.Content>
								<Select.Item value="__auto__" label="Automático">Automático</Select.Item>
								{#each effectiveModulePaths as ep}
									<Select.Item value={ep} label={ep}>
										<span class="block max-w-[min(100vw-4rem,36rem)] truncate font-mono text-xs">{ep}</span>
									</Select.Item>
								{/each}
							</Select.Content>
						</Select.Root>
					</div>
					<Button
						variant="secondary"
						size="sm"
						class="shrink-0 sm:mb-0.5"
						disabled={pkcsBusy || pathsBusy}
						onclick={() => refreshPkcs11Listing()}
					>
						Actualizar
					</Button>
				</div>

				{#if preferredChoice === "__auto__"}
					<p class="text-muted-foreground text-xs leading-relaxed">
						{#if autoResolvedModulePath}
							Se está usando:
							<code class="bg-muted ml-1 rounded px-1.5 py-0.5 font-mono text-[11px]">{autoResolvedModulePath}</code>
						{:else}
							Todavía no hay ningún controlador activo: conecta el lector con el DNIe o la tarjeta dentro,
							o añade abajo la ruta del controlador del fabricante.
						{/if}
					</p>
				{/if}

				<div class="space-y-3 border-t pt-4">
					<div>
						<p class="text-sm font-medium">Controladores instalados (rutas)</p>
						<p class="text-muted-foreground mt-1 text-xs leading-relaxed">
							Rutas en disco del controlador del lector que NexoSign probará en orden. La primera fila se usa
							primero con «Automático». Añade rutas absolutas y ordénalas arrastrando por el asa.
						</p>
					</div>

					{#if pkcs11Paths.length > 0}
						<div class="select-none" bind:this={pathListEl}>
							<Table.Root>
								<Table.Header>
									<Table.Row>
										<Table.Head class="w-10 px-2">
											<span class="sr-only">Ordenar</span>
										</Table.Head>
										<Table.Head class="w-10">#</Table.Head>
										<Table.Head>Ruta</Table.Head>
										<Table.Head class="w-14 text-right">
											<span class="sr-only">Eliminar</span>
										</Table.Head>
									</Table.Row>
								</Table.Header>
								<Table.Body>
									{#each pkcs11Paths as p, i}
										{@const dragging = pathDrag?.armed && pathDrag.fromIndex === i}
										{@const showAbove = pathDrag?.armed && pathDropIndex === i}
										{@const showBelow = pathDrag?.armed && i === pkcs11Paths.length - 1 && pathDropIndex === pkcs11Paths.length}
										<Table.Row
											data-path-row
											class={cn(
												"transition-opacity",
												dragging && "opacity-30",
												showAbove && "border-primary border-t-2",
												showBelow && "border-primary border-b-2",
											)}
										>
											<Table.Cell class="w-10 px-2 align-middle">
												<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
												<span
													class="text-muted-foreground hover:text-foreground inline-flex cursor-grab touch-none select-none active:cursor-grabbing"
													class:cursor-not-allowed={pathsBusy}
													class:pointer-events-none={pathsBusy}
													role="button"
													tabindex="0"
													aria-label="Arrastrar para cambiar prioridad"
													onpointerdown={(e) => startPathDrag(e, i)}
												>
													<GripVerticalIcon class="size-4 shrink-0" />
												</span>
											</Table.Cell>
											<Table.Cell class="text-muted-foreground align-middle">{i + 1}</Table.Cell>
											<Table.Cell class="max-w-0 align-middle">
												<code class="block truncate text-xs" title={p}>{p}</code>
											</Table.Cell>
											<Table.Cell class="text-right align-middle">
												<Button
													variant="ghost"
													size="icon-sm"
													class="text-destructive size-8"
													disabled={pathsBusy}
													aria-label="Quitar ruta"
													onclick={() => removeDriverPathRow(p)}
												>
													<Trash2Icon class="size-4" />
												</Button>
											</Table.Cell>
										</Table.Row>
									{/each}
								</Table.Body>
							</Table.Root>
						</div>
					{:else}
						<p class="text-muted-foreground text-sm">No hay rutas guardadas (se usarán las incluidas por defecto).</p>
					{/if}

					<div class="flex flex-col gap-2 sm:flex-row sm:items-end">
						<div class="grid min-w-0 flex-1 gap-2">
							<Label for="new-p11-path">Añadir ruta del controlador (archivo del fabricante del lector)</Label>
							<Input
								id="new-p11-path"
								bind:value={newDriverPath}
								placeholder="/usr/local/lib/ejemplo-pkcs11.so"
								disabled={pathsBusy}
								onkeydown={(e) => e.key === "Enter" && addDriverPathRow()}
							/>
						</div>
						<Button class="sm:mb-0.5" disabled={pathsBusy || !newDriverPath.trim()} onclick={() => addDriverPathRow()}>
							Añadir
						</Button>
					</div>

					<Button variant="outline" size="sm" disabled={pathsBusy} onclick={() => resetDriverPathsDefaults()}>
						Restaurar lista por defecto
					</Button>
				</div>
			</Card.Content>
		</Card.Root>

		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Diagnóstico del lector</Card.Title>
				<Card.Description>
					Comprueba qué ve NexoSign en el lector. Si en Certificados no aparece tu DNIe, ejecuta el
					diagnóstico aquí (sin abrir ReFirma) y revisa el controlador PKCS#11 más abajo. En Certificados,
					«Origen» indica chip o Windows; si es el mismo certificado en ambos sitios, solo se muestra el del lector.
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="flex flex-wrap gap-2">
					<Button variant="outline" size="sm" disabled={pkcsBusy} onclick={() => diagnosePkcs11Slots()}>
						Ejecutar diagnóstico
					</Button>
					<Button variant="outline" size="sm" disabled={pkcsBusy} onclick={() => probeChipCertificates()}>
						Explorar certificados en chip
					</Button>
					<Button
						variant="outline"
						size="sm"
						disabled={pkcsBusy}
						onclick={async () => {
							pkcsBusy = true;
							try {
								await pkcs11.pkcs11ResetConnection();
								pkcsDiag = null;
								toast.success("Conexión PKCS#11 reiniciada");
							} catch (e) {
								toast.error(String(e));
							} finally {
								pkcsBusy = false;
							}
						}}
					>
						Reinicializar lector
					</Button>
				</div>
				{#if pkcsDiag}
					<p class="text-muted-foreground break-all text-xs" title="Controlador PKCS#11 activo para diagnóstico">
						Controlador: <code class="bg-muted rounded px-1">{pkcsDiag.module_path}</code>
					</p>
					<div class="text-muted-foreground flex flex-wrap gap-2 text-xs">
						<Badge variant="outline" title="Puertos del lector detectados por el controlador.">
							Puertos detectados: {pkcsDiag.count_pkcs11_get_slot_list_true}
						</Badge>
						<Badge variant="secondary" title="Puertos con DNIe o tarjeta insertados listos para firmar.">
							Con tarjeta lista: {pkcsDiag.count_effective_for_nexosign}
						</Badge>
					</div>
					<ScrollArea.Root class="h-[220px] rounded-md border">
						<div class="p-4">
							<ul class="space-y-2 text-sm">
								{#each pkcsDiag.slots as s}
									<li class="border-b pb-2 last:border-0">
										<span class="font-medium">Puerto {s.slot_id}</span>
										· {s.slot_description.trim()}
										<span class="text-muted-foreground">
											· {s.token_present_in_slot_info ? "con tarjeta" : "vacío"}</span
										>
										{#if s.token_label}
											<div class="text-muted-foreground text-xs">{s.token_label}</div>
										{/if}
									</li>
								{/each}
							</ul>
						</div>
					</ScrollArea.Root>
				{/if}
				{#if pkcsCertProbe}
					<div class="space-y-2">
						<p class="text-muted-foreground text-xs">
							Por controlador: X.509 en chip (raw) vs certificados de firma (nonRepudiation). Si raw=0 con
							tarjeta insertada, prueba «Probar con PIN» en Certificados.
						</p>
						<ScrollArea.Root class="max-h-[280px] rounded-md border">
							<div class="p-3">
								<Table.Root>
									<Table.Header>
										<Table.Row>
											<Table.Head class="text-xs">Controlador / slot</Table.Head>
											<Table.Head class="text-right text-xs">X.509</Table.Head>
											<Table.Head class="text-right text-xs">Firma</Table.Head>
										</Table.Row>
									</Table.Header>
									<Table.Body>
										{#each pkcsCertProbe.modules as mod}
											<Table.Row>
												<Table.Cell colspan={3} class="bg-muted/40 py-1.5 font-mono text-[10px]">
													{mod.path}
													{#if mod.error}
														<span class="text-destructive ml-2">— {mod.error}</span>
													{/if}
												</Table.Cell>
											</Table.Row>
											{#each mod.slots as slot}
												<Table.Row>
													<Table.Cell class="text-xs">
														Slot {slot.slot_id}
														{#if slot.token_label}
															<span class="text-muted-foreground"> · {slot.token_label}</span>
														{/if}
														{#if slot.session_error}
															<div class="text-destructive text-[10px]">{slot.session_error}</div>
														{/if}
													</Table.Cell>
													<Table.Cell class="text-right text-xs">{slot.raw_x509_count}</Table.Cell>
													<Table.Cell class="text-right text-xs">{slot.signing_after_filter_count}</Table.Cell>
												</Table.Row>
											{/each}
											{#if mod.slots.length === 0}
												<Table.Row>
													<Table.Cell colspan={3} class="text-muted-foreground text-xs">
														Sin slots con tarjeta
													</Table.Cell>
												</Table.Row>
											{/if}
										{/each}
									</Table.Body>
								</Table.Root>
							</div>
						</ScrollArea.Root>
					</div>
				{/if}
			</Card.Content>
		</Card.Root>
	{/if}

	<Card.Root>
		<Card.Header class="flex flex-row flex-wrap items-start justify-between gap-2 space-y-0">
			<div>
				<Card.Title class="text-base">Servicio local</Card.Title>
				<Card.Description>Solo útil si algo falla al firmar desde una web.</Card.Description>
			</div>
			<Button variant="outline" size="sm" onclick={() => refreshDiagnostics()} disabled={diagLoading}>
				{diagLoading ? "Comprobando…" : "Comprobar de nuevo"}
			</Button>
		</Card.Header>
		<Card.Content class="space-y-3 text-sm">
			<code class="bg-muted block rounded-md p-2 text-xs">{apiUrl || "…"}</code>
			{#if diagLoading && health === null && pingOk === null}
				<p class="text-muted-foreground">Comprobando…</p>
			{:else if health}
				<div class="flex flex-wrap items-center gap-2">
					<Badge variant="secondary">{health.service}</Badge>
					<Badge variant="outline">v{health.version}</Badge>
					<Badge class={health.status === "ok" ? "" : "bg-destructive"}>{health.status}</Badge>
					<Badge variant={pingOk ? "default" : "destructive"}>
						{pingOk ? "Respuesta correcta" : "Sin respuesta"}
					</Badge>
				</div>
			{:else}
				<p class="text-destructive">No responde el servicio en esta máquina.</p>
			{/if}
			<p class="text-muted-foreground text-xs">
				Enlaces <code class="bg-muted rounded px-1">nexosign://</code> pueden abrir esta app desde el navegador.
			</p>
			{#if isTauriRuntime()}
				<div class="border-muted space-y-2 border-t pt-3">
					<p class="text-sm font-medium">Caché temporal (API local)</p>
					<p class="text-muted-foreground text-xs leading-relaxed">
						Subidas por multipart del portal y PDF firmados guardados para descarga HTTP pueden acumularse
						bajo el directorio temporal del sistema. Limpiar libera espacio; los trabajos ya expuestos por URL
						dejarán de poder descargarse hasta una nueva firma.
					</p>
					<Button
						type="button"
						variant="outline"
						size="sm"
						disabled={cacheClearing}
						onclick={() => void confirmClearApiCache()}
					>
						{cacheClearing ? "Limpiando…" : "Limpiar caché del servicio local"}
					</Button>
				</div>
			{/if}
		</Card.Content>
	</Card.Root>

	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">Sitios permitidos</Card.Title>
			<Card.Description>
				Páginas que pueden pedir firmas a esta app (normalmente las añades cuando aparece el aviso).
			</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-6">
			{#if !isTauriRuntime()}
				<p class="text-muted-foreground text-sm">Abre NexoSign en Tauri para gestionar orígenes.</p>
			{:else}
				<div class="flex flex-col gap-3 sm:flex-row sm:items-end">
					<div class="grid w-full gap-2 sm:max-w-xl">
						<Label for="new-origin">Nuevo origen</Label>
						<Input
							id="new-origin"
							bind:value={newOrigin}
							placeholder="https://portal.ejemplo.com"
							onkeydown={(e) => e.key === "Enter" && add()}
						/>
					</div>
					<Button class="sm:mb-0.5" onclick={() => add()} disabled={!newOrigin.trim() || loading}>
						Añadir
					</Button>
				</div>

				{#if loading && origins.length === 0}
					<p class="text-muted-foreground text-sm">Cargando…</p>
				{:else if origins.length === 0}
					<p class="text-muted-foreground text-sm">No hay orígenes extra en base de datos.</p>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head>Origen</Table.Head>
								<Table.Head class="w-[100px] text-right">Acciones</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each origins as o}
								<Table.Row>
									<Table.Cell class="font-mono text-xs">{o}</Table.Cell>
									<Table.Cell class="text-right">
										<Button variant="ghost" size="sm" onclick={() => remove(o)}>Quitar</Button>
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				{/if}
			{/if}
		</Card.Content>
	</Card.Root>
</div>

{#if pathDrag?.armed}
	<div
		class="border-primary bg-card text-card-foreground pointer-events-none fixed z-60 max-w-md -translate-x-1/2 -translate-y-1/2 truncate rounded-md border px-2 py-1 font-mono text-[11px] shadow-lg"
		style="left: {pathGhostX}px; top: {pathGhostY}px;"
		aria-hidden="true"
	>
		{pathDrag.label}
	</div>
{/if}

<style>
	:global(body.nexo-dnd-active) {
		cursor: grabbing !important;
		user-select: none;
	}
	:global(body.nexo-dnd-active *) {
		cursor: grabbing !important;
	}
</style>
