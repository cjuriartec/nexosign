<script lang="ts">
	import { tick } from "svelte";
	import * as Dialog from "$lib/components/ui/dialog/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import { Badge } from "$lib/components/ui/badge/index.js";
	import { cn } from "$lib/utils.js";
	import KeyRoundIcon from "@lucide/svelte/icons/key-round";
	import EyeIcon from "@lucide/svelte/icons/eye";
	import EyeOffIcon from "@lucide/svelte/icons/eye-off";
	import Loader2Icon from "@lucide/svelte/icons/loader-2";
	import CircleCheckIcon from "@lucide/svelte/icons/circle-check";

	interface Props {
		open?: boolean;
		pin?: string;
		pinError?: string | null;
		pinVisible?: boolean;
		disabled?: boolean;
		submitInFlight?: boolean;
		signLabel?: string;
		onConfirm?: () => void;
		class?: string;
	}

	let {
		open = $bindable(false),
		pin = $bindable(""),
		pinError = $bindable(null),
		pinVisible = $bindable(false),
		disabled = false,
		submitInFlight = false,
		signLabel = "Firmar",
		onConfirm,
		class: className,
	}: Props = $props();

	let pinInputRef = $state<HTMLInputElement | null>(null);
	const pinReady = $derived(pin.trim().length > 0);
	const canConfirm = $derived(!disabled && !submitInFlight && pinReady);

	$effect(() => {
		if (!open) return;
		void tick().then(() => pinInputRef?.focus());
	});

	function confirm() {
		if (!canConfirm) return;
		void onConfirm?.();
	}
</script>

<div class={cn("shrink-0", className)}>
	<button
		type="button"
		class="border-border/60 hover:bg-muted/40 flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left transition-colors disabled:opacity-50"
		disabled={disabled}
		onclick={() => {
			open = true;
		}}
	>
		<KeyRoundIcon class="text-muted-foreground size-3.5 shrink-0" aria-hidden="true" />
		<span class="min-w-0 flex-1 text-xs">
			<span class="font-medium">PIN para firmar</span>
			<span class="text-muted-foreground">
				{#if pinReady}
					· introducido
				{:else}
					· pendiente
				{/if}
			</span>
		</span>
		{#if pinReady}
			<Badge variant="secondary" class="h-5 shrink-0 gap-1 px-1.5 text-[10px]">
				<CircleCheckIcon class="size-3" aria-hidden="true" />
				Listo
			</Badge>
		{:else}
			<Badge variant="outline" class="text-muted-foreground h-5 shrink-0 text-[10px]">
				Requerido
			</Badge>
		{/if}
	</button>

	<Dialog.Root bind:open>
		<Dialog.Content class="sm:max-w-xs" showCloseButton={!submitInFlight}>
			<Dialog.Header>
				<Dialog.Title>PIN del certificado</Dialog.Title>
				<Dialog.Description>
					Introduce el PIN de tu DNIe o tarjeta para firmar el lote.
				</Dialog.Description>
			</Dialog.Header>

			<div class="space-y-1.5">
				<Label for="sign-pin-modal" class="text-xs font-medium">PIN</Label>
				<div class="relative">
					<Input
						id="sign-pin-modal"
						bind:ref={pinInputRef}
						type={pinVisible ? "text" : "password"}
						autocomplete="off"
						bind:value={pin}
						placeholder="••••"
						class={cn("h-10 pr-10", pinError ? "border-destructive" : "")}
						disabled={disabled || submitInFlight}
						oninput={() => {
							pinError = null;
						}}
						onkeydown={(e) => {
							if (e.key === "Enter" && canConfirm) {
								e.preventDefault();
								confirm();
							}
						}}
					/>
					<Button
						type="button"
						variant="ghost"
						size="icon"
						class="text-muted-foreground absolute right-0.5 top-1/2 size-8 -translate-y-1/2"
						disabled={disabled || submitInFlight}
						aria-label={pinVisible ? "Ocultar PIN" : "Mostrar PIN"}
						onclick={() => {
							pinVisible = !pinVisible;
						}}
					>
						{#if pinVisible}
							<EyeOffIcon class="size-4" />
						{:else}
							<EyeIcon class="size-4" />
						{/if}
					</Button>
				</div>
				{#if pinError}
					<p class="text-destructive text-xs font-medium leading-snug">{pinError}</p>
				{/if}
			</div>

			<Dialog.Footer class="gap-2 sm:justify-end">
				<Button
					type="button"
					variant="outline"
					class="w-full sm:w-auto"
					disabled={submitInFlight}
					onclick={() => {
						open = false;
					}}
				>
					Cancelar
				</Button>
				<Button
					type="button"
					class="w-full gap-2 sm:w-auto"
					disabled={!canConfirm}
					onclick={() => confirm()}
				>
					{#if submitInFlight}
						<Loader2Icon class="size-4 animate-spin" aria-hidden="true" />
						Firmando…
					{:else}
						{signLabel}
					{/if}
				</Button>
			</Dialog.Footer>
		</Dialog.Content>
	</Dialog.Root>
</div>
