<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Table from "$lib/components/ui/table/index.js";
	import * as pkcs11 from "$lib/tauri/pkcs11";
	import type { SigningCertSummary } from "$lib/tauri/pkcs11";
	import { isPkcs11NoTokenError } from "$lib/tauri/pkcs11-errors";
	import { isTauriRuntime } from "$lib/tauri/env";
	import SignatureAppearanceCard from "$lib/components/signature-appearance-card.svelte";
	import { getHumanNameFromDn, extractDniFromDn, extractPurposeFromDn } from "$lib/signature-appearance";

	let certs = $state<SigningCertSummary[]>([]);
	let busy = $state(false);

	async function loadCerts() {
		if (!isTauriRuntime()) return;
		busy = true;
		try {
			certs = await pkcs11.listSigningCertificates();
		} catch (e) {
			certs = [];
			if (isPkcs11NoTokenError(e)) {
				return;
			}
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	onMount(() => {
		if (!isTauriRuntime()) return;
		void loadCerts();
	});
</script>

<svelte:head>
	<title>Certificados — NexoSign</title>
</svelte:head>

<div class="mx-auto max-w-6xl space-y-8">
	<div>
		<h1 class="text-3xl font-semibold tracking-tight">Certificados para firma</h1>
		<p class="text-muted-foreground mt-1 max-w-2xl text-sm">
			Certificados del token que puedes seleccionar al firmar. El PIN solo lo pedimos en el flujo de firma.
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
		<Card.Root>
			<Card.Header class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
				<div>
					<Card.Title class="text-base">Certificados</Card.Title>
					<Card.Description>
						Listado actual desde tu tarjeta o token.
					</Card.Description>
				</div>
				<Button variant="outline" size="sm" class="shrink-0 self-start" disabled={busy} onclick={() => loadCerts()}>
					Recargar
				</Button>
			</Card.Header>
			<Card.Content>
				{#if certs.length === 0}
					<p class="text-muted-foreground text-sm">No hay certificados.</p>
				{:else}
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head>Titular</Table.Head>
								<Table.Head>Documento</Table.Head>
								<Table.Head class="text-right">Uso</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each certs as c}
								<Table.Row>
									<Table.Cell class="font-medium">{getHumanNameFromDn(c.subject_dn) || c.label || "—"}</Table.Cell>
									<Table.Cell class="text-muted-foreground text-sm">{extractDniFromDn(c.subject_dn) || "—"}</Table.Cell>
									<Table.Cell class="text-right">
										<span class="inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 border-transparent bg-secondary text-secondary-foreground hover:bg-secondary/80">
											{extractPurposeFromDn(c.subject_dn)}
										</span>
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				{/if}
			</Card.Content>
		</Card.Root>

	{/if}

	<Card.Root class="border-border/50 shadow-none">
		<Card.Content class="min-w-0 p-3 md:p-4">
			<SignatureAppearanceCard certs={isTauriRuntime() ? certs : []} />
		</Card.Content>
	</Card.Root>
</div>
