import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

/**
 * Components, the Reka UI wrappers included, style with the semantic colours of `base.css` only.
 * A Tailwind palette class (`bg-zinc-800`, `text-white`, `bg-black/40`) or an arbitrary colour (`bg-[#282a36]`)
 * would not follow the theme, so none may appear in a template.
 */
const PALETTE_CLASS =
	/(?:^|[\s"'`:[])(?:bg|text|border|outline|ring|fill|stroke|from|via|to|decoration|divide|accent|caret|shadow|placeholder)-(?:black|white|slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)(?:-\d{2,3})?(?:\/\d{1,3})?(?=[\s"'`]|$)/;
const ARBITRARY_COLOUR = /-\[(?:#[0-9a-f]{3,8}|rgb|hsl|oklch)/i;

const SRC = join(import.meta.dirname, "../..");

function vueFiles(): string[] {
	return readdirSync(SRC, { recursive: true, encoding: "utf8" })
		.filter((file) => file.endsWith(".vue"))
		.map((file) => join(SRC, file));
}

describe("component templates", () => {
	const files = vueFiles();

	it("exist", () => {
		expect(files.length).toBeGreaterThan(5);
	});

	for (const file of files) {
		it(`${file.slice(SRC.length + 1)} uses semantic colours only`, () => {
			const template = readFileSync(file, "utf8")
				.split("\n")
				.filter((line) => !line.trimStart().startsWith("//") && !line.includes("<!--"));
			const offending = template.filter((line) => PALETTE_CLASS.test(line) || ARBITRARY_COLOUR.test(line));
			expect(offending).toEqual([]);
		});
	}
});
