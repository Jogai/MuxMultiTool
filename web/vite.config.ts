// Rationale per plugin: docs/frontend.md.
import { readFileSync } from "node:fs";
import path from "node:path";

import tailwindcss from "@tailwindcss/vite";
import VueI18n from "@intlify/unplugin-vue-i18n/vite";
import Vue from "@vitejs/plugin-vue";
import { fontless } from "fontless";
import RekaResolver from "reka-ui/resolver";
import AutoImport from "unplugin-auto-import/vite";
import IconsResolver from "unplugin-icons/resolver";
import Icons from "unplugin-icons/vite";
import TurboConsole from "unplugin-turbo-console/vite";
import Components from "unplugin-vue-components/vite";
import { defineConfig, type Plugin, type UserConfig } from "vite";
import Info from "vite-plugin-meta-info";
import VueDevTools from "vite-plugin-vue-devtools";
import { VueRouterAutoImports } from "vue-router/unplugin";
import VueRouter from "vue-router/vite";

import phosphor from "@iconify-json/ph/icons.json" with { type: "json" };

import pkg from "./package.json" with { type: "json" };

const VUE_FILE_REGEX = /\.vue$/;
const VUE_QUERY_REGEX = /\.vue\?vue/;

/**
 * Own icons, used as `<i-mmt-*>`. Combine is Phosphor's tree-view turned a quarter to the left, then
 * mirrored left to right, so its branches read as several sources joining into one output.
 */
const MMT_ICONS: Record<string, string> = {
	combine: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256"><g transform="translate(256 0) scale(-1 1) rotate(-90 128 128)">${phosphor.icons["tree-view"].body}</g></svg>`
};

// The axum server in development (`cargo run -p server`)
const BACKEND = "http://127.0.0.1:8080";

/** The lock file entries this build reads: `dev` marks a package only the build needs. */
interface LockPackage {
	name?: string;
	version?: string;
	license?: string;
	dev?: boolean;
}

/**
 * Writes `sbom.web.json` into the build output: every production package of the lock file (the
 * non-dev closure, what can end up in the bundle) with its name, version and license. The server
 * merges it into the CycloneDX document it serves at `/sbom.cdx.json` (see development.md).
 */
function webSbom(lockFile: string): Plugin {
	return {
		name: "mmt:web-sbom",
		apply: "build",
		generateBundle() {
			const lock = JSON.parse(readFileSync(lockFile, "utf8")) as { packages?: Record<string, LockPackage> };
			const seen = new Set<string>();
			const packages = [];
			for (const [location, entry] of Object.entries(lock.packages ?? {})) {
				// The root entry is the app itself; dev packages are not bundled
				if (location === "" || entry.dev || !entry.version) continue;
				const name = entry.name ?? location.slice(location.lastIndexOf("node_modules/") + "node_modules/".length);
				const key = `${name}@${entry.version}`;
				if (seen.has(key)) continue;
				seen.add(key);
				packages.push({ name, version: entry.version, ...(entry.license ? { license: entry.license } : {}) });
			}
			packages.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
			this.emitFile({ type: "asset", fileName: "sbom.web.json", source: JSON.stringify({ packages }) });
		}
	};
}

export default defineConfig((): UserConfig => {
	return {
		// Shown by the About screen; commit and build time come from the Info() plugin below
		define: { __APP_VERSION__: JSON.stringify(pkg.version) },

		build: {
			outDir: "./dist", // Ensure the output is always in 'dist'; served by axum and embedded by Tauri
			emptyOutDir: true, // Remove existing files before the build starts
			cssMinify: "lightningcss",
			target: "esnext",
			reportCompressedSize: false,
			rolldownOptions: {
				logLevel: "silent",
				experimental: {
					lazyBarrel: true
				}
			}
		},

		css: {
			transformer: "lightningcss"
		},

		resolve: {
			alias: { "~": path.resolve(import.meta.dirname, "src") }
		},

		// The frontend always calls its own origin; in development Vite forwards to the backend.
		// The media and download routes carry an id after the prefix (`/media/{id}/…`, `/files/{id}`),
		// so the prefix alone is not proxied: `/files` is also the Files page
		server: {
			proxy: {
				"/rpc": BACKEND,
				"^/media/": BACKEND,
				"^/files/": BACKEND,
				"/sbom.cdx.json": BACKEND
			}
		},

		plugins: [
			// https://router.vuejs.org/guide/migration/v4-to-v5 (file-based routing, formerly unplugin-vue-router)
			// Must come before Vue()
			VueRouter({
				routesFolder: "src/pages",
				dts: "src/typed-router.d.ts"
			}),

			// https://devtools.vuejs.org/guide/vite-plugin
			VueDevTools(),

			// https://github.com/unplugin/unplugin-turbo-console
			TurboConsole({
				highlight: {
					themeDetect: true
				}
			}),

			Vue({
				include: [VUE_FILE_REGEX]
			}),

			// https://github.com/unplugin/unplugin-auto-import
			AutoImport({
				imports: ["vue", "pinia", "vue-i18n", "@vueuse/core", VueRouterAutoImports],
				dts: "src/auto-imports.d.ts",
				dirs: ["src/core/composables", "src/core/stores", "src/core/libs"],
				vueTemplate: true
			}),

			// https://github.com/unplugin/unplugin-icons
			Icons({
				compiler: "vue3",
				customCollections: { mmt: MMT_ICONS },
				autoInstall: false // icon set is a normal dependency, so CI installs from the lockfile only
			}),

			// https://github.com/unplugin/unplugin-vue-components
			Components({
				dirs: ["src/core/components", "src/features/*/components"],
				resolvers: [
					IconsResolver({ customCollections: ["mmt"] }),
					// https://reka-ui.com/docs/overview/installation: <RekaDialogRoot>, <RekaSelectTrigger>, …
					RekaResolver({ prefix: "Reka" })
				],
				extensions: ["vue"],
				include: [VUE_FILE_REGEX, VUE_QUERY_REGEX],
				dts: "src/components.d.ts",
				directoryAsNamespace: true, // check the resulting names, see frontend.md
				syncMode: "overwrite"
			}),

			tailwindcss(),

			// https://github.com/unjs/fontaine/tree/main/packages/fontless
			// Fonts must be self-hosted at build time: the desktop app and air-gapped containers are offline
			fontless({
				priority: ["bunny"],
				families: [
					{ name: "Atkinson Hyperlegible", provider: "bunny", weights: [400, 700], styles: ["normal"], preload: true },
					{ name: "Atkinson Hyperlegible Mono", provider: "bunny", weights: [400], styles: ["normal"] }
				]
			}),

			// https://github.com/intlify/bundle-tools/tree/main/packages/unplugin-vue-i18n
			VueI18n({
				runtimeOnly: true,
				compositionOnly: true,
				fullInstall: true,
				include: [path.resolve(import.meta.dirname, "locales/**")]
			}),

			// https://github.com/bazingaedward/vite-plugin-buildInfo
			// Version, commit and build time for the About section and status bar
			Info(),

			// The npm packages in the bundle, for the SBOM the server serves
			webSbom(path.resolve(import.meta.dirname, "package-lock.json"))
		]
	};
});
