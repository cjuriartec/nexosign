<script lang="ts">
	import { onMount } from "svelte";
	import { invoke } from "@tauri-apps/api/core";
	import {
		fetchHealth,
		fetchPing,
		requestDemoProgress,
	} from "$lib/api/local-api";
	import { subscribeProgress } from "$lib/events/progress";

	let apiStatus = $state<"idle" | "ok" | "error">("idle");
	let apiDetail = $state("");
	let lastProgress = $state<string | null>(null);
	let greetMsg = $state("");

	onMount(() => {
		let unlisten: (() => void) | undefined;

		void (async () => {
			try {
				const un = await subscribeProgress((p) => {
					lastProgress = `${p.actual}/${p.total} (${p.job_id})`;
				});
				unlisten = un;
			} catch {
				/* entorno sin Tauri */
			}

			try {
				const h = await fetchHealth();
				apiStatus = "ok";
				apiDetail = `${h.service} v${h.version}`;
			} catch (e) {
				apiStatus = "error";
				apiDetail = String(e);
			}
		})();

		return () => {
			unlisten?.();
		};
	});

	async function ping() {
		try {
			const p = await fetchPing();
			apiDetail = `ping ok=${p.ok}`;
		} catch (e) {
			apiDetail = String(e);
		}
	}

	async function demoHttpProgress() {
		const r = await requestDemoProgress();
		apiDetail = JSON.stringify(r);
	}

	async function demoCmdProgress() {
		try {
			await invoke("demo_emit_progress");
		} catch (e) {
			apiDetail = String(e);
		}
	}

	async function greet(ev: Event) {
		ev.preventDefault();
		greetMsg = await invoke<string>("greet", { name: "NexoSign" });
	}
</script>

<main class="wrap">
	<h1>NexoSign</h1>
	<p class="sub">Fase 1 — API local y eventos en tiempo real</p>

	<section class="card">
		<h2>API local (127.0.0.1:14500)</h2>
		<p data-testid="api-status" class="status" data-state={apiStatus}>
			{apiStatus === "idle"
				? "…"
				: apiStatus === "ok"
					? `✓ ${apiDetail}`
					: `✗ ${apiDetail}`}
		</p>
		<div class="row">
			<button type="button" onclick={ping}>Ping POST</button>
			<button type="button" onclick={demoHttpProgress}>Demo progreso (HTTP)</button>
			<button type="button" onclick={demoCmdProgress}>Demo progreso (comando)</button>
		</div>
	</section>

	<section class="card">
		<h2>Último evento <code>progreso</code></h2>
		<p data-testid="progress-line">{lastProgress ?? "—"}</p>
	</section>

	<section class="card">
		<form onsubmit={greet}>
			<button type="submit">Greet (IPC demo)</button>
		</form>
		<p>{greetMsg}</p>
	</section>
</main>

<style>
	.wrap {
		max-width: 42rem;
		margin: 2rem auto;
		padding: 0 1rem;
		font-family:
			system-ui,
			sans-serif;
	}
	h1 {
		font-size: 1.75rem;
		margin-bottom: 0.25rem;
	}
	.sub {
		color: #666;
		margin-bottom: 1.5rem;
	}
	.card {
		border: 1px solid #ddd;
		border-radius: 8px;
		padding: 1rem;
		margin-bottom: 1rem;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-top: 0.75rem;
	}
	button {
		padding: 0.4rem 0.75rem;
		border-radius: 6px;
		border: 1px solid #ccc;
		background: #fafafa;
		cursor: pointer;
	}
	button:hover {
		background: #eee;
	}
	code {
		font-size: 0.9em;
	}
</style>
