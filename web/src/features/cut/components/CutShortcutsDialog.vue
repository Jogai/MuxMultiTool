<script setup lang="ts">
import { formatSegmentShortcut, formatTimelineShortcut, SEGMENT_SHORTCUT_ROWS, TIMELINE_ACTIONS } from "../lib/shortcuts";

/** The timeline's fixed shortcuts and the keys of a focused segment; the player's are in its own list (the keyboard button over the picture). */
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
				<RekaDialogTitle class="text-lg font-bold">{{ t("cut.shortcuts.title") }}</RekaDialogTitle>
				<RekaDialogDescription class="mt-1 text-sm text-muted">{{ t("cut.shortcuts.body") }}</RekaDialogDescription>
				<table class="mt-4 w-full text-sm">
					<tbody>
						<tr v-for="action in TIMELINE_ACTIONS" :key="action" class="border-t border-border">
							<th scope="row" class="py-1.5 pr-3 text-left font-normal">{{ t(`cut.shortcuts.actions.${action}`) }}</th>
							<td class="py-1.5 text-right">
								<kbd class="rounded border border-border bg-surface-sunken px-1.5 py-0.5 font-mono text-xs">{{
									formatTimelineShortcut(action, { mac })
								}}</kbd>
							</td>
						</tr>
						<tr class="border-t border-border">
							<th scope="row" class="py-1.5 pr-3 text-left font-normal">{{ t("cut.shortcuts.wheelZoom") }}</th>
							<td class="py-1.5 text-right">
								<kbd class="rounded border border-border bg-surface-sunken px-1.5 py-0.5 font-mono text-xs"
									>{{ mac ? "⌘" : "Ctrl" }} + {{ t("cut.shortcuts.wheel") }}</kbd
								>
							</td>
						</tr>
						<tr class="border-t border-border">
							<th scope="row" class="py-1.5 pr-3 text-left font-normal">{{ t("cut.shortcuts.dragFree") }}</th>
							<td class="py-1.5 text-right">
								<kbd class="rounded border border-border bg-surface-sunken px-1.5 py-0.5 font-mono text-xs">Alt</kbd>
							</td>
						</tr>
						<tr class="border-t border-border">
							<th scope="colgroup" colspan="2" class="pt-3 pb-1 text-left text-xs text-muted">{{ t("cut.shortcuts.segment.title") }}</th>
						</tr>
						<tr v-for="row in SEGMENT_SHORTCUT_ROWS" :key="row.key" class="border-t border-border">
							<th scope="row" class="py-1.5 pr-3 text-left font-normal">{{ t(`cut.shortcuts.segment.${row.key}`) }}</th>
							<td class="py-1.5 text-right">
								<template v-for="(action, index) in row.actions" :key="action">
									<span v-if="index > 0"> / </span>
									<kbd class="rounded border border-border bg-surface-sunken px-1.5 py-0.5 font-mono text-xs">{{
										formatSegmentShortcut(action, { mac })
									}}</kbd>
								</template>
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
