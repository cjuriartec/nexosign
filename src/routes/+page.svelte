<script lang="ts">
	import { onMount } from "svelte";
	import { fetchHealth, fetchPing } from "$lib/api/local-api";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { getLocalApiBaseUrl } from "$lib/tauri/settings";
	import { toast } from "svelte-sonner";
	import ActivityIcon from "@lucide/svelte/icons/activity";
	import RouterIcon from "@lucide/svelte/icons/router";

	let apiBase = $state("");
	let health = $state<{ status: string; service: string; version: string } | null>(
		null,
	);
	let pingOk = $state<boolean | null>(null);
	let session = $state<Awaited<ReturnType<typeof pkcs11.pkcs11SessionStatus>> | null>(
		null,
	);
	let loading = $state(true);

	async function refresh() {
		loading = true;
		try {
			apiBase = isTauriRuntime()
				? await getLocalApiBaseUrl()
				: "http://127.0.0.1:14500";
			health = await fetchHealth(apiBase);
			const p = await fetchPing(apiBase);
			pingOk = p.ok;
			if (isTauriRuntime()) {
				session = await pkcs11.pkcs11SessionStatus();
			} else {
				session = null;
			}
		} catch (e) {
			toast.error(`API local: ${String(e)}`);
			health = null;
			pingOk = false;
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		void refresh();
	});
</script>

<svelte:head>
	<title>Inicio — NexoSign</title>
</svelte:head>

<div class="space-y-8">
	<div>
		<h1 class="text-foreground text-3xl font-semibold tracking-tight">Panel</h1>
		<p class="text-muted-foreground mt-1 max-w-2xl text-sm leading-relaxed">
			Firma PAdES con DNIe u OpenSC. La API local en loopback recibe peticiones solo desde
			orígenes de confianza.
		</p>
	</div>

	<div class="grid gap-4 md:grid-cols-2">
		<Card.Root>
			<Card.Header class="flex flex-row items-start justify-between space-y-0 pb-2">
				<div>
					<Card.Title class="text-base">API local</Card.Title>
					<Card.Description>
						<code class="bg-muted rounded px-1.5 py-0.5 text-xs">{apiBase || "…"}</code>
					</Card.Description>
				</div>
				<RouterIcon class="text-muted-foreground size-5" />
			</Card.Header>
			<Card.Content class="space-y-3">
				{#if loading}
					<p class="text-muted-foreground text-sm">Comprobando…</p>
				{:else if health}
					<div class="flex flex-wrap items-center gap-2">
						<Badge variant="secondary">{health.service}</Badge>
						<Badge variant="outline">v{health.version}</Badge>
						<Badge class={health.status === "ok" ? "" : "bg-destructive"}>
							{health.status}
						</Badge>
						<Badge variant={pingOk ? "default" : "destructive"}>
							ping {pingOk ? "ok" : "fallo"}
						</Badge>
					</div>
				{:else}
					<p class="text-destructive text-sm">Sin conexión con el servidor local.</p>
				{/if}
				<Button variant="outline" size="sm" onclick={() => refresh()}>Actualizar</Button>
			</Card.Content>
		</Card.Root>

		<Card.Root>
			<Card.Header class="flex flex-row items-start justify-between space-y-0 pb-2">
				<div>
					<Card.Title class="text-base">PKCS#11</Card.Title>
					<Card.Description>Sesión del token para firma</Card.Description>
				</div>
				<ActivityIcon class="text-muted-foreground size-5" />
			</Card.Header>
			<Card.Content>
				{#if !isTauriRuntime()}
					<p class="text-muted-foreground text-sm">
						Ejecuta la app con Tauri para usar el lector y certificados.
					</p>
				{:else if loading}
					<p class="text-muted-foreground text-sm">Cargando sesión…</p>
				{:else if session}
					<div class="space-y-2 text-sm">
						<p>
							Estado:
							<strong>{session.logged_in ? "Sesión activa" : "Sin iniciar sesión"}</strong>
						</p>
						<p class="text-muted-foreground">
							Timeout inactividad: {session.idle_timeout_secs}s
							{#if session.seconds_until_auto_logout != null}
								· cierre automático en ~{session.seconds_until_auto_logout}s
							{/if}
						</p>
					</div>
				{/if}
				<div class="mt-4 flex flex-wrap gap-2">
					<Button href="/certificates" variant="secondary" size="sm">Certificados</Button>
					<Button href="/sign" size="sm">Firmar PDFs</Button>
				</div>
			</Card.Content>
		</Card.Root>
	</div>
</div>
