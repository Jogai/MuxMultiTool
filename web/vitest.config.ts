import Vue from "@vitejs/plugin-vue";
import { defineConfig } from "vitest/config";

// Separate from vite.config.ts: the dev-only plugins start servers that keep the test process alive
export default defineConfig({
	plugins: [Vue()],
	test: {
		include: ["src/**/*.test.ts"]
	}
});
