/// <reference types="vite/client" />
/// <reference types="vite-plugin-vue-layouts-next/client" />
/// <reference types="unplugin-icons/types/vue" />

// Stub for the TS 7 check, which can't read .vue files. vue-tsc ignores it and checks the real components.
declare module "*.vue" {
	import type { DefineComponent } from "vue";
	const component: DefineComponent<object, object, unknown>;
	export default component;
}

/** The version from package.json, defined in vite.config.ts and vitest.config.ts. */
declare const __APP_VERSION__: string;
