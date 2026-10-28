<script setup lang="ts">
import { formatShortcut, PLAYER_SHORTCUTS, SEEK_STEP_SECONDS } from "../lib/shortcuts";

/** The fixed shortcut list, opened from the player's keyboard button or with `?`. */
const open = defineModel<boolean>("open", { default: false });
const { t } = useI18n();
const mac = computed(() => /mac/i.test(globalThis.navigator?.platform ?? ""));
</script>

<template>
	<RekaDialogRoot v-model:open="open">
		<RekaDialogPortal>
			<RekaDialogOverlay class="fixed inset-0 bg-overlay" />
			<RekaDialogContent
				class="fixed top-1/2 left-1/2 w-[min(28rem,calc(100vw-2rem))] -translate-1/2 rounded-lg border border-border bg-surface p-5 shadow-lg"
			>
				<RekaDialogTitle class="text-lg font-bold">{{ t("player.shortcuts.title") }}</RekaDialogTitle>
				<RekaDialogDescription class="mt-1 text-sm text-muted">{{ t("player.shortcuts.body") }}</RekaDialogDescription>
				<table class="mt-4 w-full text-sm">
					<tbody>
						<tr v-for="shortcut in PLAYER_SHORTCUTS" :key="shortcut.action" class="border-t border-border">
							<th scope="row" class="py-1.5 pr-3 text-left font-normal">
								{{ t(`player.shortcuts.actions.${shortcut.action}`, { seconds: SEEK_STEP_SECONDS }) }}
							</th>
							<td class="py-1.5 text-right">
								<kbd class="rounded border border-border bg-surface-sunken px-1.5 py-0.5 font-mono text-xs">
									{{ formatShortcut(shortcut, { mac }) }}
								</kbd>
							</td>
						</tr>
					</tbody>
				</table>
				<div class="mt-5 flex justify-end">
					<RekaDialogClose
						class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent"
					>
						{{ t("dialogs.close") }}
					</RekaDialogClose>
				</div>
			</RekaDialogContent>
		</RekaDialogPortal>
	</RekaDialogRoot>
</template>
