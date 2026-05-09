<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import * as Table from "$lib/components/ui/table/index.js";
	import {
		addAllowedOrigin,
		listAllowedOrigins,
		removeAllowedOrigin,
		getLocalApiBaseUrl,
	} from "$lib/tauri/settings";
	import { isTauriRuntime } from "$lib/tauri/env";
	import { LOCAL_API_BASE } from "$lib/config/constants";

	let origins = $state<string[]>([]);
	let newOrigin = $state("");
	let apiUrl = $state(LOCAL_API_BASE);
	let loading = $state(false);

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
		if (isTauriRuntime()) void refresh();
	});
</script>

<svelte:head>
	<title>Ajustes — NexoSign</title>
</svelte:head>

<div class="mx-auto max-w-3xl space-y-8">
	<div>
		<h1 class="text-3xl font-semibold tracking-tight">Ajustes</h1>
		<p class="text-muted-foreground mt-1 text-sm">
			Orígenes permitidos para CORS y para <code class="bg-muted rounded px-1 text-xs">POST /batch/sign</code>.
			Los valores por defecto de desarrollo siguen activos; aquí persistes aprobaciones extra en SQLite.
		</p>
	</div>

	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">API local</Card.Title>
			<Card.Description>
				Base URL expuesta por la app (solo loopback).
				{#if isTauriRuntime()}
					<code class="bg-muted mt-2 block rounded-md p-2 text-xs">{apiUrl}</code>
				{:else}
					<code class="bg-muted mt-2 block rounded-md p-2 text-xs">{LOCAL_API_BASE}</code>
				{/if}
			</Card.Description>
		</Card.Header>
		<Card.Content class="text-muted-foreground space-y-2 text-sm">
			<p>
				Enlaces <code class="bg-muted rounded px-1">nexosign://</code> abren la vista de firma cuando el SO
				reenvía la URL a la app (macOS/iOS/Android; en Windows/Linux puede abrirse una nueva instancia).
			</p>
		</Card.Content>
	</Card.Root>

	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">Orígenes HTTP(S)</Card.Title>
			<Card.Description>
				Normalizados a esquema + host + puerto. Sin reiniciar el servidor: la lista en memoria se actualiza al
				guardar.
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
