<script lang="ts">
	import { page } from "$app/state";
	import NexoLogo from "@lucide/svelte/icons/shield-check";
	import LayoutDashboardIcon from "@lucide/svelte/icons/layout-dashboard";
	import FilePenLineIcon from "@lucide/svelte/icons/file-pen-line";
	import IdCardIcon from "@lucide/svelte/icons/id-card";
	import SettingsIcon from "@lucide/svelte/icons/settings";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";

	const nav = [
		{ href: "/", label: "Inicio", icon: LayoutDashboardIcon },
		{ href: "/sign", label: "Firmar PDFs", icon: FilePenLineIcon },
		{ href: "/certificates", label: "Certificados", icon: IdCardIcon },
		{ href: "/settings", label: "Ajustes", icon: SettingsIcon },
	];
</script>

<Sidebar.Root collapsible="icon">
	<Sidebar.Header class="border-b border-sidebar-border px-2 py-3">
		<div class="flex items-center gap-2 px-2">
			<div
				class="bg-sidebar-primary text-sidebar-primary-foreground flex size-8 items-center justify-center rounded-lg"
			>
				<NexoLogo class="size-5" />
			</div>
			<div class="grid flex-1 text-left text-sm leading-tight group-data-[collapsible=icon]:hidden">
				<span class="truncate font-semibold">NexoSign</span>
				<span class="text-muted-foreground truncate text-xs">DNIe · PAdES</span>
			</div>
		</div>
	</Sidebar.Header>
	<Sidebar.Content>
		<Sidebar.Group>
			<Sidebar.GroupLabel>Navegación</Sidebar.GroupLabel>
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
	<Sidebar.Rail />
</Sidebar.Root>
