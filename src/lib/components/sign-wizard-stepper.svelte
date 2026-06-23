<script lang="ts">
	import type { Component } from "svelte";
	import { SIGN_STEPS, TOTAL_STEPS } from "$lib/sign/constants";
	import { cn } from "$lib/utils.js";
	import CheckIcon from "@lucide/svelte/icons/check";
	import FilesIcon from "@lucide/svelte/icons/files";
	import ListChecksIcon from "@lucide/svelte/icons/list-checks";

	const STEP_ICONS: Record<number, Component> = {
		1: FilesIcon,
		2: ListChecksIcon,
	};

	interface Props {
		currentStep: number;
		isStepDisabled: (step: number) => boolean;
		onStepClick: (step: number) => void;
		class?: string;
	}

	let { currentStep, isStepDisabled, onStepClick, class: className }: Props = $props();
</script>

<nav class={cn("w-full min-w-0", className)} aria-label="Pasos de firma">
	<ol class="flex items-center gap-0">
		{#each SIGN_STEPS as s, i (s.step)}
			{@const done = currentStep > s.step}
			{@const active = currentStep === s.step}
			{@const disabled = isStepDisabled(s.step)}
			{@const Icon = STEP_ICONS[s.step]}
			<li class="flex min-w-0 flex-1 items-center last:flex-none">
				<button
					type="button"
					disabled={disabled}
					title={s.title}
					aria-label={`${s.title}${active ? ", actual" : done ? ", completado" : ""}`}
					aria-current={active ? "step" : undefined}
					onclick={() => onStepClick(s.step)}
					class={cn(
						"relative flex size-8 shrink-0 items-center justify-center rounded-full border transition-colors sm:size-9",
						active && "border-primary bg-primary text-primary-foreground shadow-sm",
						done && !active && "border-primary/30 bg-primary/10 text-primary hover:bg-primary/15",
						!done && !active && "border-border/80 bg-background text-muted-foreground",
						disabled && "pointer-events-none opacity-40",
					)}
				>
					{#if done}
						<CheckIcon class="size-3.5 sm:size-4" aria-hidden="true" />
					{:else if Icon}
						<Icon class="size-3.5 sm:size-4" aria-hidden="true" />
					{/if}
				</button>
				<span
					class={cn(
						"ml-1.5 hidden truncate text-xs font-medium sm:inline",
						active ? "text-foreground" : "text-muted-foreground",
					)}
				>
					{s.title}
				</span>
				{#if i < TOTAL_STEPS - 1}
					<div
						class={cn(
							"mx-1 h-0.5 min-w-[12px] flex-1 rounded-full sm:mx-2",
							currentStep > s.step ? "bg-primary/70" : "bg-border",
						)}
						aria-hidden="true"
					></div>
				{/if}
			</li>
		{/each}
	</ol>
</nav>
