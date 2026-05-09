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

	let p11Path = $state<string | null>(null);
	let p11Err = $state<string | null>(null);
	let slotCount = $state<number | null>(null);
	let p11Diag = $state<{
		module_path: string;
		count_pkcs11_get_slot_list_true: number;
		count_effective_for_nexosign: number;
		slots: {
			slot_id: number;
			slot_description: string;
			manufacturer_id: string;
			token_present_in_slot_info: boolean;
			token_label: string | null;
		}[];
	} | null>(null);
	let signingCerts = $state<
		{ id_hex: string; label: string; subject_dn: string }[]
	>([]);
	let pin = $state("");
	let sessionInfo = $state<{
		logged_in: boolean;
		idle_timeout_secs: number;
		seconds_until_auto_logout: number | null;
	} | null>(null);

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

		void refreshSession();

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

	async function refreshSession() {
		try {
			sessionInfo = await invoke<{
				logged_in: boolean;
				idle_timeout_secs: number;
				seconds_until_auto_logout: number | null;
			}>("pkcs11_session_status");
		} catch {
			sessionInfo = null;
		}
	}

	async function probeP11() {
		p11Err = null;
		try {
			p11Path = await invoke<string>("probe_pkcs11_module_path");
		} catch (e) {
			p11Path = null;
			p11Err = String(e);
		}
	}

	async function countSlots() {
		p11Err = null;
		p11Diag = null;
		try {
			const d = await invoke<{
				module_path: string;
				count_pkcs11_get_slot_list_true: number;
				count_effective_for_nexosign: number;
				slots: {
					slot_id: number;
					slot_description: string;
					manufacturer_id: string;
					token_present_in_slot_info: boolean;
					token_label: string | null;
				}[];
			}>("pkcs11_diagnose_slots");
			p11Diag = d;
			slotCount = d.count_effective_for_nexosign;
		} catch (e) {
			slotCount = null;
			p11Err = String(e);
		}
	}

	async function listSigning() {
		p11Err = null;
		try {
			signingCerts = await invoke("list_signing_certificates");
		} catch (e) {
			signingCerts = [];
			p11Err = String(e);
		}
		await refreshSession();
	}

	async function loginP11() {
		p11Err = null;
		try {
			await invoke("pkcs11_login", { pin });
		} catch (e) {
			p11Err = String(e);
		}
		pin = "";
		await refreshSession();
	}

	async function logoutP11() {
		p11Err = null;
		try {
			await invoke("pkcs11_logout");
		} catch (e) {
			p11Err = String(e);
		}
		await refreshSession();
	}

	async function greet(ev: Event) {
		ev.preventDefault();
		greetMsg = await invoke<string>("greet", { name: "NexoSign" });
	}
</script>

<main class="wrap">
	<h1>NexoSign</h1>
	<p class="sub">Fase 1 — API local y eventos · Fase 2 — PKCS#11 / DNIe (probar con lector y OpenSC)</p>

	<section class="card">
		<h2>PKCS#11 (DNIe / OpenSC)</h2>
		<div class="row">
			<button type="button" onclick={probeP11}>Módulo detectado</button>
			<button type="button" onclick={countSlots}>Diagnóstico PKCS#11</button>
			<button type="button" onclick={listSigning}>Certificados de firma</button>
		</div>
		{#if p11Path}
			<p class="mono">Módulo: {p11Path}</p>
		{/if}
		{#if p11Err}
			<p class="err">{p11Err}</p>
		{/if}
		{#if slotCount !== null}
			<p>
				Slots con token (lista PKCS#11 estricta): <strong>{p11Diag?.count_pkcs11_get_slot_list_true ?? "—"}</strong>
				· Slots utilizables por NexoSign: <strong>{slotCount}</strong>
			</p>
			{#if p11Diag && p11Diag.count_pkcs11_get_slot_list_true === 0 && p11Diag.count_effective_for_nexosign > 0}
				<p class="hint ok">
					El driver devuelve <strong>0</strong> en la lista PKCS#11 «con token», pero el lector marca
					<code>token_present=true</code>; NexoSign usa ese segundo criterio (comportamiento habitual en algunos lectores).
				</p>
			{/if}
		{/if}
		{#if p11Diag && p11Diag.slots.length === 0}
			<p class="hint">
				Este PKCS#11 no lista ningún slot: revisa lector USB/CCID, OpenSC/pcscd o usa el <code>.dylib</code>
				del middleware oficial del DNIe con <code>NEXOSIGN_PKCS11_MODULE</code>.
			</p>
		{/if}
		{#if p11Diag?.slots?.length}
			<ul class="slots">
				{#each p11Diag.slots as s}
					{@const desc = s.slot_description.trim()}
					{@const man = s.manufacturer_id.trim()}
					<li class="mono">
						id={s.slot_id}
						{desc}{#if man} · {man}{/if}
						· token_present={s.token_present_in_slot_info}
						{#if s.token_label != null && s.token_label.trim() !== ""}
							· label="{s.token_label.trim()}"
						{/if}
					</li>
				{/each}
			</ul>
			{#if p11Diag.count_effective_for_nexosign === 0 && p11Diag.slots.some((s) => !s.token_present_in_slot_info)}
				<p class="hint">
					El lector puede estar visible, pero este <strong>módulo PKCS#11</strong> no ve tarjeta insertada.
					Prueba el middleware del DNIe (FNMT/CCN) y exporta
					<code>NEXOSIGN_PKCS11_MODULE</code> con la ruta a su <code>.dylib</code>/<code>.so</code>.
				</p>
			{/if}
		{/if}
		{#if signingCerts.length}
			<ul class="certs">
				{#each signingCerts as c}
					<li>
						<strong>{c.label || "(sin label)"}</strong> — {c.subject_dn}
						<br />
						<span class="mono">id: {c.id_hex}</span>
					</li>
				{/each}
			</ul>
		{/if}
		<form
			class="row"
			onsubmit={(e) => {
				e.preventDefault();
				void loginP11();
			}}
		>
			<input
				type="password"
				autocomplete="off"
				placeholder="PIN"
				bind:value={pin}
			/>
			<button type="submit">Login token</button>
			<button type="button" onclick={logoutP11}>Logout</button>
		</form>
		{#if sessionInfo}
			<p data-testid="p11-session">
				Sesión: {sessionInfo.logged_in ? "logueada" : "no logueada"} · timeout
				{sessionInfo.idle_timeout_secs}s
				{#if sessionInfo.seconds_until_auto_logout != null}
					· auto-logout en ~{sessionInfo.seconds_until_auto_logout}s
				{/if}
			</p>
		{/if}
	</section>

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
	code {
		font-size: 0.9em;
	}
	.mono {
		font-size: 0.85rem;
		word-break: break-all;
	}
	.err {
		color: #a00;
	}
	.certs {
		margin: 0.5rem 0 0 1rem;
	}
	.slots {
		margin: 0.5rem 0 0 1rem;
		padding-left: 1rem;
		font-size: 0.85rem;
	}
	.hint.ok {
		background: #e8f5e9;
	}
	input {
		padding: 0.35rem 0.5rem;
		border-radius: 6px;
		border: 1px solid #ccc;
		min-width: 8rem;
	}
</style>
