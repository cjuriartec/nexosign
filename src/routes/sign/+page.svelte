<script lang="ts">
	import { onMount } from "svelte";
	import { open } from "@tauri-apps/plugin-dialog";
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
	import { postBatchSign } from "$lib/api/local-api";
	import { subscribeProgress, type ProgressPayload } from "$lib/events/progress";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import { cancelBatchJob, getLocalApiBaseUrl } from "$lib/tauri/settings";
	import { isTauriRuntime } from "$lib/tauri/env";
	import FileStackIcon from "@lucide/svelte/icons/files";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";

	let paths = $state<string[]>([]);
	let certs = $state<SigningCertSummary[]>([]);
	let certId = $state("");
	let pin = $state("");
	let jobId = $state(`job-${Date.now()}`);
	let session = $state<Awaited<ReturnType<typeof pkcs11.pkcs11SessionStatus>> | null>(null);
	let apiBase = $state("");
	let busy = $state(false);

	let activeJobId = $state<string | null>(null);
	/** Ref para el callback de progreso (evita cierre obsoleto). */
	const activeJobRef: { current: string | null } = { current: null };
	let progressPct = $state(0);
	let logLines = $state<string[]>([]);

	function pushLog(line: string) {
		logLines = [...logLines, line].slice(-120);
	}

	async function refreshSession() {
		if (!isTauriRuntime()) return;
		session = await pkcs11.pkcs11SessionStatus();
	}

	async function refreshCerts() {
		if (!isTauriRuntime()) return;
		try {
			certs = await pkcs11.listSigningCertificates();
			if (certs.length && !certId) {
				certId = certs[0]?.id_hex ?? "";
			}
		} catch (e) {
			toast.error(String(e));
			certs = [];
		}
	}

	async function loginPin() {
		if (!isTauriRuntime() || !pin.trim()) return;
		busy = true;
		try {
			await pkcs11.pkcs11Login(pin);
			toast.success("Sesión PKCS#11 iniciada");
			await refreshSession();
			await refreshCerts();
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
		const unique = [...new Set([...paths, ...list])];
		paths = unique;
		toast.message(`${list.length} archivo(s) añadidos`);
	}

	function removeAt(i: number) {
		paths = paths.filter((_, idx) => idx !== i);
	}

	async function submitBatch() {
		if (!certId.trim()) {
			toast.error("Selecciona un certificado.");
			return;
		}
		if (paths.length === 0) {
			toast.error("Añade al menos un PDF.");
			return;
		}
		if (!session?.logged_in) {
			toast.error("Inicia sesión con el PIN del token primero.");
			return;
		}

		busy = true;
		logLines = [];
		progressPct = 0;
		activeJobId = null;
		activeJobRef.current = null;
		try {
			const jid = jobId.trim() || undefined;
			const res = await postBatchSign(
				{
					cert_id_hex: certId.trim(),
					inputs: paths,
					job_id: jid,
				},
				apiBase,
			);
			activeJobId = res.job_id;
			activeJobRef.current = res.job_id;
			toast.success(`Lote encolado: ${res.job_id}`);
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	async function cancelJob() {
		const id = activeJobId;
		if (!id) {
			toast.message("No hay un job activo reciente.");
			return;
		}
		if (!isTauriRuntime()) return;
		try {
			const ok = await cancelBatchJob(id);
			toast.message(ok ? "Cancelación enviada" : "Job no encontrado en cola");
		} catch (e) {
			toast.error(String(e));
		}
	}

	onMount(() => {
		let unlisten: (() => void) | undefined;

		void (async () => {
			if (isTauriRuntime()) {
				apiBase = await getLocalApiBaseUrl();
				await refreshSession();
				if (session?.logged_in) await refreshCerts();
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

	// sync ref si se limpia el job en UI
	$effect(() => {
		activeJobRef.current = activeJobId;
	});
</script>

<svelte:head>
	<title>Firmar PDFs — NexoSign</title>
</svelte:head>

<div class="space-y-8">
	<div>
		<h1 class="text-3xl font-semibold tracking-tight">Firma masiva PAdES</h1>
		<p class="text-muted-foreground mt-1 max-w-2xl text-sm leading-relaxed">
			Selecciona PDFs en disco (rutas absolutas), certificado de firma y cola el lote contra la API local. El
			progreso llega por eventos en tiempo real.
		</p>
	</div>

	{#if !isTauriRuntime()}
		<Alert>
			<FileStackIcon class="size-4" />
			<AlertTitle>Vista limitada</AlertTitle>
			<AlertDescription>
				Para elegir archivos y usar PKCS#11, ejecuta NexoSign como aplicación Tauri. En navegador solo puedes
				probar la API si ya tienes otra instancia sirviendo en {apiBase}.
			</AlertDescription>
		</Alert>
	{/if}

	<div class="grid gap-4 lg:grid-cols-2">
		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Token y certificado</Card.Title>
				<Card.Description>PIN y certificado de firma listados por PKCS#11.</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				{#if session}
					<p class="text-sm">
						Sesión:
						<strong>{session.logged_in ? "activa" : "sin PIN"}</strong>
					</p>
				{/if}
				<div class="grid gap-2">
					<Label for="pin-sign">PIN del token</Label>
					<div class="flex flex-wrap gap-2">
						<Input
							id="pin-sign"
							class="max-w-xs"
							type="password"
							autocomplete="off"
							bind:value={pin}
							placeholder="PIN"
						/>
						<Button variant="secondary" disabled={busy || !pin.trim()} onclick={() => loginPin()}>
							Aplicar PIN
						</Button>
						<Button variant="outline" size="sm" disabled={busy} onclick={() => refreshCerts()}>
							Refrescar lista
						</Button>
					</div>
				</div>

				<div class="grid gap-2">
					<Label>Certificado</Label>
					{#if certs.length === 0}
						<p class="text-muted-foreground text-sm">
							Inicia sesión y refresca la lista en «Certificados».
						</p>
					{:else}
						<Select.Root type="single" bind:value={certId}>
							<Select.Trigger class="w-full max-w-lg justify-between">
								{certs.find((c) => c.id_hex === certId)?.label ||
									"Selecciona un certificado"}
							</Select.Trigger>
							<Select.Content>
								{#each certs as c}
									<Select.Item value={c.id_hex} label={c.label}>
										<div class="flex flex-col text-left">
											<span class="font-medium">{c.label || "(sin etiqueta)"}</span>
											<span class="text-muted-foreground text-xs">{c.subject_dn}</span>
										</div>
									</Select.Item>
								{/each}
							</Select.Content>
						</Select.Root>
					{/if}
				</div>
			</Card.Content>
		</Card.Root>

		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Archivos PDF</Card.Title>
				<Card.Description>Solo rutas absolutas en este equipo.</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-3">
				<div class="flex flex-wrap gap-2">
					<Button onclick={() => pickPdfs()} disabled={busy}>
						<FileStackIcon class="mr-2 size-4" />
						Añadir PDFs…
					</Button>
					<Button variant="outline" disabled={busy || paths.length === 0} onclick={() => (paths = [])}>
						Vaciar lista
					</Button>
				</div>
				{#if paths.length === 0}
					<p class="text-muted-foreground text-sm">Todavía no hay archivos.</p>
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
				{/if}
			</Card.Content>
		</Card.Root>
	</div>

	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">Cola de firma</Card.Title>
			<Card.Description>
				<code class="bg-muted rounded px-1 text-xs">job_id</code> opcional (si no, el servidor genera uno).
			</Card.Description>
		</Card.Header>
		<Card.Content class="flex flex-col gap-4 sm:flex-row sm:items-end">
			<div class="grid max-w-md flex-1 gap-2">
				<Label for="job">job_id</Label>
				<Input id="job" bind:value={jobId} placeholder="job-opcional" />
			</div>
			<div class="flex flex-wrap gap-2">
				<Button
					disabled={busy || paths.length === 0 || !certId}
					onclick={() => submitBatch()}
				>
					Encolar firma
				</Button>
				<Button variant="outline" disabled={busy} onclick={() => cancelJob()}>Cancelar job</Button>
			</div>
		</Card.Content>
	</Card.Root>

	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">Progreso</Card.Title>
			<Card.Description>
				Evento <code class="bg-muted rounded px-1 text-xs">progreso</code> del job activo.
				{#if activeJobId}
					<span class="text-foreground font-mono text-xs"> {activeJobId}</span>
				{/if}
			</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-4">
			<Progress value={progressPct} class="h-2" />
			<ScrollArea.Root class="h-48 rounded-md border">
				<div class="p-3 font-mono text-xs leading-relaxed">
					{#if logLines.length === 0}
						<span class="text-muted-foreground">Sin eventos todavía.</span>
					{:else}
						{#each logLines as line}
							<div>{line}</div>
						{/each}
					{/if}
				</div>
			</ScrollArea.Root>
		</Card.Content>
	</Card.Root>
</div>
