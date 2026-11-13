<script setup lang="ts">
import { SBOM_PHASES, SBOM_URL, filterSbom, groupSbom } from "~/core/libs/sbom";

/**
 * The software bill of materials section of Settings: the CycloneDX document the server serves, split into
 * runtime and build time and, within each, one collapsible section per kind (programs, Rust crates, npm
 * packages) with a table of name, version and license. Every section starts collapsed. A search over name
 * and license opens the sections with matches and hides the others; clearing it brings back the sections
 * that were open before. A download link points at the document itself.
 */
const { t } = useI18n();
const sbom = useSbom();
const query = ref("");
// Filtering ~300 rows is cheap, but a debounce keeps typing smooth on a slow machine
const debouncedQuery = refDebounced(query, 150);

onMounted(() => void sbom.load());

const components = computed(() => sbom.sbom.value?.components ?? []);
const groups = computed(() => groupSbom(components.value));
const searching = computed(() => debouncedQuery.value.trim() !== "");
/** The groups with the rows the search leaves; without a search every group with all its rows. */
const shown = computed(() =>
	groups.value
		.map((group) => ({ ...group, rows: filterSbom(group.components, debouncedQuery.value) }))
		.filter((group) => !searching.value || group.rows.length > 0)
);
const phases = computed(() =>
	SBOM_PHASES.map((phase) => {
		const phaseGroups = shown.value.filter((group) => group.phase === phase);
		const total = groups.value.filter((group) => group.phase === phase).reduce((sum, group) => sum + group.components.length, 0);
		return { phase, groups: phaseGroups, total };
	}).filter((phase) => phase.groups.length > 0)
);
const fileName = computed(() => `muxmultitool-${sbom.sbom.value?.application?.version ?? "sbom"}.cdx.json`);

/** The keys of the open sections. */
const open = ref(new Set<string>());
/** What was open when a search started, restored when it is cleared. */
let openBeforeSearch: Set<string> | undefined;

watch([searching, shown], ([isSearching]) => {
	if (isSearching) {
		openBeforeSearch ??= open.value;
		open.value = new Set(shown.value.map((group) => group.key));
	} else if (openBeforeSearch) {
		open.value = openBeforeSearch;
		openBeforeSearch = undefined;
	}
});

/** Keeps `open` in step when the user opens or closes a section. */
function onToggle(key: string, event: Event) {
	const isOpen = (event.target as HTMLDetailsElement).open;
	if (isOpen === open.value.has(key)) return;
	const next = new Set(open.value);
	if (isOpen) next.add(key);
	else next.delete(key);
	open.value = next;
}
</script>

<template>
	<div>
		<p class="max-w-prose text-sm text-muted">{{ t("pages.settings.sbom.description") }}</p>

		<p v-if="sbom.state.value === 'loading' || sbom.state.value === 'idle'" class="mt-3 text-sm text-muted" role="status">
			{{ t("pages.settings.sbom.loading") }}
		</p>
		<ErrorPanel v-else-if="sbom.state.value === 'error' && sbom.error.value" :error="sbom.error.value" class="mt-3" />

		<template v-else>
			<div class="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
				<label class="flex items-center gap-2">
					<i-ph-magnifying-glass class="size-4 text-muted" aria-hidden="true" />
					<input
						v-model="query"
						type="search"
						class="w-64 rounded-md border border-border bg-surface px-2 py-1 text-text"
						:placeholder="t('pages.settings.sbom.search')"
						:aria-label="t('pages.settings.sbom.search')"
					/>
				</label>
				<span class="text-muted">{{ t("pages.settings.sbom.total", components.length) }}</span>
				<a
					:href="SBOM_URL"
					:download="fileName"
					class="ml-auto flex items-center gap-1 text-accent underline focus-visible:outline-2 focus-visible:outline-accent"
				>
					<i-ph-download-simple class="size-4" aria-hidden="true" />
					{{ t("pages.settings.sbom.download") }}
				</a>
			</div>

			<p v-if="phases.length === 0" class="mt-3 text-sm text-muted">
				{{ searching ? t("pages.settings.sbom.noMatch", { query: debouncedQuery }) : t("pages.settings.sbom.empty") }}
			</p>
			<section v-for="item in phases" :key="item.phase" class="mt-5" :aria-labelledby="`sbom-${item.phase}`">
				<h4 :id="`sbom-${item.phase}`" class="text-sm font-bold">
					{{ t(`pages.settings.sbom.phase.${item.phase}.title`) }}
					<span class="font-normal text-muted">· {{ t("pages.settings.sbom.total", item.total) }}</span>
				</h4>
				<p class="max-w-prose text-xs text-muted">{{ t(`pages.settings.sbom.phase.${item.phase}.description`) }}</p>

				<details
					v-for="group in item.groups"
					:key="group.key"
					:open="open.has(group.key)"
					class="group mt-2 rounded-md border border-border"
					@toggle="onToggle(group.key, $event)"
				>
					<summary
						class="flex cursor-pointer list-none items-center gap-2 rounded-md px-3 py-2 text-sm hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent [&::-webkit-details-marker]:hidden"
					>
						<i-ph-caret-right class="size-4 text-muted transition-transform group-open:rotate-90" aria-hidden="true" />
						<span>{{ t(`pages.settings.sbom.group.${group.kind}`) }}</span>
						<span class="ml-auto text-xs text-muted tabular-nums">
							<template v-if="searching">{{
								t("pages.settings.sbom.matches", { shown: group.rows.length, total: group.components.length })
							}}</template>
							<template v-else>{{ group.components.length }}</template>
						</span>
					</summary>
					<!-- Rendered only while open: a closed build-time list costs nothing -->
					<table v-if="open.has(group.key)" class="w-full border-collapse text-sm">
						<thead class="text-left text-xs text-muted uppercase">
							<tr class="border-y border-border">
								<th scope="col" class="py-2 pr-3 pl-3 font-normal">{{ t("pages.settings.sbom.columns.name") }}</th>
								<th scope="col" class="py-2 pr-3 font-normal">{{ t("pages.settings.sbom.columns.version") }}</th>
								<th scope="col" class="py-2 pr-3 font-normal">{{ t("pages.settings.sbom.columns.license") }}</th>
							</tr>
						</thead>
						<tbody>
							<tr
								v-for="row in group.rows"
								:key="`${row.name}@${row.version}`"
								class="border-b border-border last:border-b-0 hover:bg-surface-raised"
							>
								<td class="py-1.5 pr-3 pl-3 font-mono">{{ row.name }}</td>
								<td class="py-1.5 pr-3 font-mono whitespace-nowrap">{{ row.version }}</td>
								<td class="py-1.5 pr-3">
									<template v-if="row.license">{{ row.license }}</template>
									<span v-else class="text-muted">{{ t("pages.settings.sbom.noLicense") }}</span>
								</td>
							</tr>
						</tbody>
					</table>
				</details>
			</section>
			<p v-if="sbom.sbom.value?.timestamp" class="mt-4 text-xs text-muted">
				{{ t("pages.settings.sbom.built", { time: sbom.sbom.value.timestamp }) }}
			</p>
		</template>
	</div>
</template>
