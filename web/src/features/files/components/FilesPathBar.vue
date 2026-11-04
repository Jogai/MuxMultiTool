<script setup lang="ts">
import type { Entry } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * The bar above the browser: the breadcrumbs of the current folder, the wildcard selection
 * (pattern, "Select matching", "Include subfolders") and "Open files…", which goes through the
 * shell's file picker so that the desktop app can answer it with a native dialog.
 */
const { breadcrumbs, busy = false, canFind = true } = defineProps<{ breadcrumbs: Entry[]; busy?: boolean; canFind?: boolean }>();
const emit = defineEmits<{ navigate: [id: string]; find: [patterns: string[], recursive: boolean]; open: [] }>();
const { t } = useI18n();

const pattern = ref("*.mkv");
const recursive = ref(false);
const patterns = computed(() => splitPatterns(pattern.value));

function submit() {
	if (patterns.value.length > 0) {
		emit("find", patterns.value, recursive.value);
	}
}
</script>

<template>
	<div class="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-border px-4 py-2">
		<nav :aria-label="t('files.breadcrumb')" class="flex min-w-0 flex-1 items-center gap-0.5 font-mono text-sm">
			<button
				type="button"
				class="rounded px-1 py-0.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
				:class="breadcrumbs.length === 0 ? 'text-text' : 'text-muted'"
				:aria-current="breadcrumbs.length === 0 ? 'location' : undefined"
				@click="emit('navigate', '')"
			>
				{{ t("files.roots") }}
			</button>
			<template v-for="(crumb, index) in breadcrumbs" :key="crumb.id">
				<i-ph-caret-right class="size-3.5 shrink-0 text-muted" aria-hidden="true" />
				<button
					type="button"
					class="truncate rounded px-1 py-0.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					:class="index === breadcrumbs.length - 1 ? 'text-text' : 'text-muted'"
					:aria-current="index === breadcrumbs.length - 1 ? 'location' : undefined"
					@click="emit('navigate', crumb.id)"
				>
					{{ crumb.name }}
				</button>
			</template>
		</nav>

		<form class="flex flex-wrap items-center gap-2 text-sm" @submit.prevent="submit">
			<label class="flex items-center gap-2">
				<span class="text-muted">{{ t("files.pattern.label") }}</span>
				<ShellTooltip :text="t('files.pattern.hint')">
					<input
						v-model="pattern"
						type="text"
						class="w-40 rounded-md border border-border bg-surface px-2 py-1 font-mono focus-visible:outline-2 focus-visible:outline-accent"
						:placeholder="t('files.pattern.placeholder')"
						spellcheck="false"
					/>
				</ShellTooltip>
			</label>
			<button
				type="submit"
				class="flex items-center gap-1 rounded-md border border-border px-3 py-1 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50"
				:disabled="busy || !canFind || patterns.length === 0"
			>
				<i-ph-magnifying-glass class="size-3.5" aria-hidden="true" />
				{{ t("files.pattern.select") }}
			</button>
			<label class="flex items-center gap-1.5">
				<input v-model="recursive" type="checkbox" class="accent-accent" />
				{{ t("files.pattern.recursive") }}
			</label>
		</form>

		<button
			type="button"
			class="flex items-center gap-1.5 rounded-md bg-accent px-3 py-1 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50"
			:disabled="busy"
			@click="emit('open')"
		>
			<i-ph-folder-open class="size-4" aria-hidden="true" />
			{{ t("files.open") }}
		</button>
	</div>
</template>
