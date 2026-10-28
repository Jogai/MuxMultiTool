/// <reference types="vite/client" />
/// <reference types="vite-plugin-vue-layouts-next/client" />
/// <reference types="unplugin-icons/types/vue" />

// Stub for the TS 7 check, which can't read .vue files. vue-tsc ignores it and checks the real components.
declare module "*.vue" {
	import type { DefineComponent } from "vue";
	const component: DefineComponent<object, object, unknown>;
	export default component;
}

// hls.js ships one declaration file for both builds but only maps it to the default entry
declare module "hls.js/light" {
	export * from "hls.js";
	export { default } from "hls.js";
}

/** The version from package.json, defined in vite.config.ts and vitest.config.ts. */
declare const __APP_VERSION__: string;
