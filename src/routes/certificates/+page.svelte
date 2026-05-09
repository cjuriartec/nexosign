<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import * as Table from "$lib/components/ui/table/index.js";
	import * as ScrollArea from "$lib/components/ui/scroll-area/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { Pkcs11Diagnostics, SigningCertSummary } from "$lib/tauri/pkcs11";
	import { isTauriRuntime } from "$lib/tauri/env";
	import CpuIcon from "@lucide/svelte/icons/cpu";
	import KeyRoundIcon from "@lucide/svelte/icons/key-round";

	let modulePath = $state<string | null>(null);
	let diag = $state<Pkcs11Diagnostics | null>(null);
	let certs = $state<SigningCertSummary[]>([]);
	let pin = $state("");
	let session = $state<Awaited<ReturnType<typeof pkcs11.pkcs11SessionStatus>> | null>(null);
	let busy = $state(false);

	async function refreshSession() {
		if (!isTauriRuntime()) return;
		session = await pkcs11.pkcs11SessionStatus();
	}

	async function probe() {
		if (!isTauriRuntime()) return;
		busy = true;
		try {
			modulePath = await pkcs11.probePkcs11ModulePath();
			toast.success("Módulo detectado");
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	async function diagnose() {
		if (!isTauriRuntime()) return;
		busy = true;
		try {
			diag = await pkcs11.pkcs11DiagnoseSlots();
			toast.success("Diagnóstico PKCS#11 listo");
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	async function loadCerts() {
		if (!isTauriRuntime()) return;
		busy = true;
		try {
			certs = await pkcs11.listSigningCertificates();
		} catch (e) {
			toast.error(String(e));
			certs = [];
		} finally {
			busy = false;
		}
	}

	async function login() {
		if (!isTauriRuntime()) return;
		busy = true;
		try {
			await pkcs11.pkcs11Login(pin);
			pin = "";
			toast.success("Sesión PKCS#11 iniciada");
			await refreshSession();
			await loadCerts();
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	async function logout() {
		if (!isTauriRuntime()) return;
		busy = true;
		try {
			await pkcs11.pkcs11Logout();
			certs = [];
			toast.message("Sesión cerrada");
			await refreshSession();
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	onMount(() => {
		if (!isTauriRuntime()) return;
		void refreshSession();
		void probe();
	});
</script>

<svelte:head>
	<title>Certificados — NexoSign</title>
</svelte:head>

<div class="space-y-8">
	<div>
		<h1 class="text-3xl font-semibold tracking-tight">Certificados PKCS#11</h1>
		<p class="text-muted-foreground mt-1 max-w-2xl text-sm">
			Detecta el middleware, revisa slots y abre sesión con el PIN del token para listar
			certificados de firma.
		</p>
	</div>

	{#if !isTauriRuntime()}
		<Card.Root>
			<Card.Header>
				<Card.Title>Solo escritorio</Card.Title>
				<Card.Description>
					Esta vista requiere la aplicación Tauri con acceso al controlador PKCS#11.
				</Card.Description>
			</Card.Header>
		</Card.Root>
	{:else}
		<div class="grid gap-4 lg:grid-cols-2">
			<Card.Root>
				<Card.Header class="flex flex-row items-start justify-between space-y-0">
					<div>
						<Card.Title class="text-base">Middleware</Card.Title>
						<Card.Description>Ruta del módulo cargado por NexoSign</Card.Description>
					</div>
					<CpuIcon class="text-muted-foreground size-5" />
				</Card.Header>
				<Card.Content class="space-y-3">
					{#if modulePath}
						<code class="bg-muted block overflow-x-auto rounded-md p-3 text-xs">{modulePath}</code>
					{:else}
						<p class="text-muted-foreground text-sm">Aún no detectado.</p>
					{/if}
					<Button variant="secondary" size="sm" disabled={busy} onclick={() => probe()}>
						Detectar de nuevo
					</Button>
				</Card.Content>
			</Card.Root>

			<Card.Root>
				<Card.Header class="flex flex-row items-start justify-between space-y-0">
					<div>
						<Card.Title class="text-base">Sesión</Card.Title>
						<Card.Description>PIN del token DNIe / tarjeta</Card.Description>
					</div>
					<KeyRoundIcon class="text-muted-foreground size-5" />
				</Card.Header>
				<Card.Content class="space-y-4">
					{#if session}
						<p class="text-sm">
							<strong>{session.logged_in ? "Activa" : "Sin PIN"}</strong>
							{#if session.seconds_until_auto_logout != null}
								<span class="text-muted-foreground">
									· auto-cierre ~{session.seconds_until_auto_logout}s</span
								>
							{/if}
						</p>
					{/if}
					<div class="space-y-2">
						<Label for="pin">PIN</Label>
						<Input
							id="pin"
							type="password"
							autocomplete="off"
							bind:value={pin}
							placeholder="••••••••"
							onkeydown={(e) => e.key === "Enter" && login()}
						/>
					</div>
					<div class="flex flex-wrap gap-2">
						<Button disabled={busy || !pin} onclick={() => login()}>Iniciar sesión</Button>
						<Button variant="outline" disabled={busy} onclick={() => logout()}>Cerrar sesión</Button>
						<Button variant="secondary" disabled={busy} onclick={() => loadCerts()}>
							Refrescar certificados
						</Button>
					</div>
				</Card.Content>
			</Card.Root>
		</div>

		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Diagnóstico de slots</Card.Title>
				<Card.Description>
					Lista PKCS#11 frente al criterio NexoSign (token presente).
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<Button variant="outline" size="sm" disabled={busy} onclick={() => diagnose()}>
					Ejecutar diagnóstico
				</Button>
				{#if diag}
					<div class="text-muted-foreground flex flex-wrap gap-2 text-xs">
						<Badge variant="outline">estrictos: {diag.count_pkcs11_get_slot_list_true}</Badge>
						<Badge variant="secondary">utilizables: {diag.count_effective_for_nexosign}</Badge>
					</div>
					<ScrollArea.Root class="h-[220px] rounded-md border">
						<div class="p-4">
							<ul class="space-y-2 text-sm">
								{#each diag.slots as s}
									<li class="border-b pb-2 last:border-0">
										<span class="font-medium">Slot {s.slot_id}</span>
										· {s.slot_description.trim()}
										<span class="text-muted-foreground">
											· token_present={s.token_present_in_slot_info}</span
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
			</Card.Content>
		</Card.Root>

		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Certificados de firma</Card.Title>
				<Card.Description>Filtrados para uso de firma digital (no solo autenticación).</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if certs.length === 0}
					<p class="text-muted-foreground text-sm">
						Inicia sesión con PIN y pulsa «Refrescar certificados».
					</p>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head>Etiqueta</Table.Head>
								<Table.Head>Subject</Table.Head>
								<Table.Head class="w-[120px] text-right">id (hex)</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each certs as c}
								<Table.Row>
									<Table.Cell class="font-medium">{c.label || "—"}</Table.Cell>
									<Table.Cell class="max-w-[280px] truncate text-xs">{c.subject_dn}</Table.Cell>
									<Table.Cell class="text-right font-mono text-xs">{c.id_hex}</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				{/if}
			</Card.Content>
		</Card.Root>
	{/if}
</div>
