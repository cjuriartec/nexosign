<script lang="ts">
	import { cn } from "$lib/utils.js";

	interface Props {
		class?: string;
		/** Centra el contenido del panel (confirmar / resultado). */
		contentCentered?: boolean;
		children: import("svelte").Snippet;
		footer?: import("svelte").Snippet;
	}

	let { class: className, contentCentered = false, children, footer }: Props = $props();
</script>

<section class={cn("flex min-h-0 w-full flex-1 flex-col", className)}>
	<div
		class="bg-card text-card-foreground flex min-h-[min(24rem,calc(100dvh-13rem))] flex-1 flex-col overflow-hidden rounded-xl border shadow-sm"
	>
		<div
			class={cn(
				"flex min-h-0 flex-1 flex-col overflow-hidden p-4 sm:p-5",
				contentCentered && "items-center justify-center",
			)}
		>
			{@render children()}
		</div>
		{#if footer}
			<div
				class="border-border/80 bg-muted/25 flex shrink-0 items-center justify-end gap-2 border-t px-4 py-3"
			>
				{@render footer()}
			</div>
		{/if}
	</div>
</section>
