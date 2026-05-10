<script lang="ts">
	import { page } from "$app/state";
	import FilePenLineIcon from "@lucide/svelte/icons/file-pen-line";
	import ListOrderedIcon from "@lucide/svelte/icons/list-ordered";
	import IdCardIcon from "@lucide/svelte/icons/id-card";
	import SettingsIcon from "@lucide/svelte/icons/settings";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";

	const nav = [
		{ href: "/sign", label: "Firmar", icon: FilePenLineIcon },
		{ href: "/queue", label: "Colas", icon: ListOrderedIcon },
		{ href: "/certificates", label: "Certificados", icon: IdCardIcon },
		{ href: "/settings", label: "Ajustes", icon: SettingsIcon },
	];
</script>

<Sidebar.Root collapsible="icon">
	<Sidebar.Header
		class="border-b border-sidebar-border px-2 py-3 group-data-[collapsible=icon]:px-0 group-data-[collapsible=icon]:py-2"
	>
		<a
			href="/sign"
			aria-label="NexoSign"
			class="flex min-w-0 items-center gap-2 rounded-md px-2 py-1 outline-none ring-sidebar-ring hover:bg-sidebar-accent/50 focus-visible:ring-2 group-data-[collapsible=icon]:justify-center group-data-[collapsible=icon]:gap-0 group-data-[collapsible=icon]:px-0"
		>
			<div
				class="flex size-8 shrink-0 overflow-hidden rounded-lg ring-1 ring-sidebar-border/50 group-data-[collapsible=icon]:size-7"
			>
				<img
					src="/favicon.png"
					alt=""
					width="24"
					height="24"
					class="size-full object-cover"
					draggable="false"
				/>
			</div>
			<div class="grid min-w-0 flex-1 text-left text-sm leading-tight group-data-[collapsible=icon]:hidden">
				<span class="truncate font-semibold">NexoSign</span>
			</div>
		</a>
	</Sidebar.Header>
	<Sidebar.Content>
		<Sidebar.Group>
			<Sidebar.GroupLabel class="sr-only">Menú</Sidebar.GroupLabel>
			<Sidebar.GroupContent>
				<Sidebar.Menu>
					{#each nav as item (item.href)}
						{@const Icon = item.icon}
						<Sidebar.MenuItem>
							<Sidebar.MenuButton
								isActive={page.url.pathname === item.href}
								tooltipContent={item.label}
							>
								{#snippet child({ props })}
									<a href={item.href} {...props}>
										<Icon />
										<span>{item.label}</span>
									</a>
								{/snippet}
							</Sidebar.MenuButton>
						</Sidebar.MenuItem>
					{/each}
				</Sidebar.Menu>
			</Sidebar.GroupContent>
		</Sidebar.Group>
	</Sidebar.Content>
	<Sidebar.Footer class="border-t border-sidebar-border p-2">
		<div class="flex justify-center group-data-[collapsible=icon]:px-0">
			<Sidebar.Trigger
				class="w-full max-w-full justify-center gap-2 group-data-[collapsible=icon]:size-8 group-data-[collapsible=icon]:p-0"
				aria-label="Contraer o expandir menú lateral"
			/>
		</div>
	</Sidebar.Footer>
</Sidebar.Root>
