<script lang="ts">
	let { children } = $props();
</script>

<div class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
	{@render children()}
</div>
