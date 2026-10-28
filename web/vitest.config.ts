import Vue from "@vitejs/plugin-vue";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// Separate from vite.config.ts: the dev-only plugins start servers that keep the test process alive
export default defineConfig({
	plugins: [Vue()],
	resolve: { alias: { "~": fileURLToPath(new URL("./src", import.meta.url)) } },
	test: {
		include: ["src/**/*.test.ts"]
	}
});
