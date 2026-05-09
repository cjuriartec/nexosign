<script lang="ts">
	import { onMount } from "svelte";
	import { invoke } from "@tauri-apps/api/core";
	import { listen } from "@tauri-apps/api/event";
	import { postBatchSign } from "$lib/api/local-api";
	import { subscribeProgress } from "$lib/events/progress";
	import type { ProgressPayload } from "$lib/events/progress";

	let certId = $state("");
	let jobId = $state(`job-${Date.now()}`);
	let pathsText = $state("");
	let logLines = $state<string[]>([]);
	let busy = $state(false);
	let lastQueuedJobId = $state<string | null>(null);
	let deepUrls = $state<string[]>([]);

	function pushLog(line: string) {
		logLines = [...logLines, line].slice(-80);
	}

	onMount(() => {
		const unsubs: (() => void)[] = [];

		void (async () => {
			try {
				unsubs.push(
					await subscribeProgress((p: ProgressPayload) => {
						const parts = [
							`${p.actual}/${p.total}`,
							p.job_id,
							p.nombre_archivo ?? "",
							p.error ? `ERR: ${p.error}` : "ok",
						];
						pushLog(parts.join(" · "));
					}),
				);
				unsubs.push(
					await listen<{ urls: string[] }>("nexosign-deep-link", (e) => {
						deepUrls = e.payload.urls ?? [];
						pushLog(`deep-link: ${deepUrls.join(", ")}`);
					}),
				);
			} catch {
				/* sin Tauri */
			}
		})();

		return () => {
			for (const u of unsubs) u();
		};
	});

	async function enqueue() {
		busy = true;
		try {
			const paths = pathsText
				.split(/\r?\n/)
				.map((s) => s.trim())
				.filter(Boolean);
			const r = await postBatchSign({
				cert_id_hex: certId.trim(),
				inputs: paths,
				job_id: jobId.trim() || undefined,
			});
			lastQueuedJobId = r.job_id;
			pushLog(`encolado job_id=${r.job_id}`);
		} catch (e) {
			pushLog(`error encolar: ${String(e)}`);
		} finally {
			busy = false;
		}
	}

	async function cancelJob() {
		const id = lastQueuedJobId?.trim();
		if (!id) {
			pushLog("cancel: define job_id encolado antes");
			return;
		}
		try {
			const stopped = await invoke<boolean>("cancel_batch_job", { job_id: id });
			pushLog(`cancel ${id}: ${stopped ? "señal enviada" : "job no activo"}`);
		} catch (e) {
			pushLog(`cancel error: ${String(e)}`);
		}
	}
</script>

<main class="wrap">
	<p class="nav"><a href="/">← Inicio</a></p>
	<h1>Firma masiva (batch)</h1>
	<p class="sub">
		Encola rutas absolutas a PDF en la API local; el progreso llega por el evento
		<code>progreso</code>. Orígenes nuevos disparan aprobación vía
		<code>origin_trust_request</code>.
	</p>

	{#if deepUrls.length}
		<section class="card">
			<h2>Último deep link</h2>
			<ul>
				{#each deepUrls as u}
					<li class="mono">{u}</li>
				{/each}
			</ul>
		</section>
	{/if}

	<section class="card">
		<h2>Encolar lote</h2>
		<label class="blk">
			<span>cert_id_hex</span>
			<input class="wide" type="text" bind:value={certId} placeholder="hex del cert en PKCS#11" />
		</label>
		<label class="blk">
			<span>job_id (opcional)</span>
			<input class="wide" type="text" bind:value={jobId} />
		</label>
		<label class="blk">
			<span>Rutas absolutas (.pdf), una por línea</span>
			<textarea class="wide" rows="6" bind:value={pathsText}></textarea>
		</label>
		<div class="row">
			<button type="button" onclick={enqueue} disabled={busy}>Encolar</button>
			<button type="button" onclick={cancelJob}>Cancelar último job</button>
		</div>
	</section>

	<section class="card">
		<h2>Registro de progreso</h2>
		<pre class="log">{logLines.length ? logLines.join("\n") : "—"}</pre>
	</section>
</main>

<style>
	.wrap {
		max-width: 44rem;
		margin: 2rem auto;
		padding: 0 1rem;
		font-family:
			system-ui,
			sans-serif;
	}
	.nav {
		margin-bottom: 0.5rem;
	}
	.nav a {
		color: #06c;
	}
	h1 {
		font-size: 1.75rem;
		margin-bottom: 0.25rem;
	}
	.sub {
		color: #555;
		margin-bottom: 1.25rem;
		line-height: 1.45;
	}
	.card {
		border: 1px solid #ddd;
		border-radius: 8px;
		padding: 1rem;
		margin-bottom: 1rem;
	}
	.blk {
		display: block;
		margin-bottom: 0.75rem;
	}
	.blk span {
		display: block;
		font-size: 0.85rem;
		margin-bottom: 0.25rem;
		color: #444;
	}
	.wide {
		width: 100%;
		box-sizing: border-box;
		padding: 0.4rem 0.5rem;
		border-radius: 6px;
		border: 1px solid #ccc;
		font-family: inherit;
	}
	textarea.wide {
		resize: vertical;
		font-family: ui-monospace, monospace;
		font-size: 0.85rem;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-top: 0.5rem;
	}
	button {
		padding: 0.45rem 0.85rem;
		border-radius: 6px;
		border: 1px solid #ccc;
		background: #fafafa;
		cursor: pointer;
	}
	button:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}
	code {
		font-size: 0.9em;
	}
	.mono {
		font-size: 0.85rem;
		word-break: break-all;
	}
	.log {
		margin: 0;
		white-space: pre-wrap;
		word-break: break-word;
		font-size: 0.8rem;
		max-height: 18rem;
		overflow: auto;
		background: #f8f8f8;
		padding: 0.75rem;
		border-radius: 6px;
	}
</style>
