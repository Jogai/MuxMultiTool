/** The nav rail entries, in order. Each has a page in `src/pages/` and a label under `nav.` in the locale. */
export const NAV_ENTRIES = [
	{ name: "files", path: "/files" },
	{ name: "streams", path: "/streams" },
	{ name: "combine", path: "/combine" },
	{ name: "cut", path: "/cut" },
	{ name: "merge", path: "/merge" },
	{ name: "crop", path: "/crop" },
	{ name: "images", path: "/images" },
	{ name: "batch", path: "/batch" },
	{ name: "presets", path: "/presets" },
	{ name: "jobs", path: "/jobs" },
	{ name: "settings", path: "/settings" }
] as const;

export type NavName = (typeof NAV_ENTRIES)[number]["name"];

/** The link to a job on the Jobs screen. */
export function jobLink(jobId: string): { path: string; query: { job: string } } {
	return { path: "/jobs", query: { job: jobId } };
}

/** The nav entry a route path belongs to, e.g. `/settings/about` → `settings`. */
export function navNameForPath(path: string): NavName | undefined {
	return NAV_ENTRIES.find((entry) => path === entry.path || path.startsWith(`${entry.path}/`))?.name;
}
