import { describe, expect, it } from "vitest";

import {
	checkTemplate,
	containerOf,
	extensionOf,
	languagesOf,
	padNumber,
	parentName,
	parseTemplate,
	placeholdersOf,
	previewFields,
	previewNames,
	renderTemplate,
	safeValue,
	stem,
	templateTarget,
	utcDate
} from "./template";

const fields = { name: "Film 1", ext: "mp4", lang: "eng-fre", date: "2026-11-04", n: "01", parent: "Inbox" };

describe("checkTemplate", () => {
	it("accepts every placeholder", () => {
		expect(checkTemplate("{n}-{parent}-{name}.{lang}.{date}.{ext}")).toBeUndefined();
	});

	it("refuses an empty template", () => {
		expect(checkTemplate("")).toEqual({ kind: "empty" });
	});

	it("refuses a path separator", () => {
		expect(checkTemplate("sub/{name}.mkv")).toEqual({ kind: "separator" });
		expect(checkTemplate("sub\\{name}.mkv")).toEqual({ kind: "separator" });
	});

	it("reports an unclosed brace with its position", () => {
		expect(checkTemplate("{name}.{ext")).toEqual({ kind: "unclosed", position: 7 });
	});

	it("reports an unknown placeholder", () => {
		expect(checkTemplate("{name}.{title}.mkv")).toEqual({ kind: "unknown", placeholder: "title" });
	});
});

describe("parseTemplate", () => {
	it("splits text and fields", () => {
		expect(parseTemplate("{n}-{name}.mkv")).toEqual([{ field: "n" }, { text: "-" }, { field: "name" }, { text: ".mkv" }]);
	});

	it("gives nothing for a bad template", () => {
		expect(parseTemplate("{nope}")).toBeUndefined();
	});

	it("lists the placeholders once each, in order", () => {
		expect(placeholdersOf("{name}.{lang}.{name}.{ext}")).toEqual(["name", "lang", "ext"]);
		expect(placeholdersOf("plain.mkv")).toEqual([]);
	});
});

describe("renderTemplate", () => {
	it("fills every placeholder in", () => {
		expect(renderTemplate("{n}-{parent}-{name}.{lang}.{date}.{ext}", fields)).toBe("01-Inbox-Film 1.eng-fre.2026-11-04.mp4");
	});

	it("keeps literal text and a template without placeholders", () => {
		expect(renderTemplate("out.mkv", fields)).toBe("out.mkv");
	});

	it("turns separators and control characters in a value into underscores", () => {
		expect(renderTemplate("{name}.mkv", { ...fields, name: "a/b\\c\td" })).toBe("a_b_c_d.mkv");
		expect(safeValue("x\u0000y")).toBe("x_y");
	});

	it("turns the characters a file name may not hold into underscores, as the server does", () => {
		expect(renderTemplate("{name}.mkv", { ...fields, name: 'a<b>c:d"e|f?g*h\u007fi' })).toBe("a_b_c_d_e_f_g_h_i.mkv");
	});

	it("renders a name Windows can write, as the server does", () => {
		const name = (template: string, name: string) => renderTemplate(template, { ...fields, name });
		// Trailing dots and spaces are dropped (Windows drops them silently)
		expect(name("{name}", "Movie. ")).toBe("Movie");
		expect(name("{name}.mkv.", "Movie")).toBe("Movie.mkv");
		expect(name("{name}", "...")).toBe("");
		// A reserved device name, bare or with an extension, is made ordinary
		expect(name("{name}", "CON")).toBe("_CON");
		expect(name("{name}.mkv", "nul")).toBe("_nul.mkv");
		expect(name("{name}.mkv", "Com1")).toBe("_Com1.mkv");
		expect(name("{name}.mkv", "LPT9 ")).toBe("_LPT9 .mkv");
		expect(name("{name}.mkv", "aux.old")).toBe("_aux.old.mkv");
		expect(name("{name}.mkv", "CON ")).toBe("_CON .mkv");
		// Not reserved: a longer name, or a device name after something else
		expect(name("{name}.mkv", "CONSOLE")).toBe("CONSOLE.mkv");
		expect(name("{name}.mkv", "COM10")).toBe("COM10.mkv");
		expect(name("{name}.mkv", "my CON")).toBe("my CON.mkv");
	});

	it("renders a template with a problem as written", () => {
		expect(renderTemplate("{name}.{bad}", fields)).toBe("{name}.{bad}");
	});
});

describe("file name helpers", () => {
	it("splits the stem and the extension", () => {
		expect(stem("Film.1.mp4")).toBe("Film.1");
		expect(extensionOf("Film.1.mp4")).toBe("mp4");
		expect(stem("noext")).toBe("noext");
		expect(extensionOf("noext")).toBe("");
		expect(stem(".hidden")).toBe(".hidden");
		expect(extensionOf(".hidden")).toBe("");
	});

	it("pads the number to the count's width", () => {
		expect(padNumber(0, 1)).toBe("1");
		expect(padNumber(0, 9)).toBe("1");
		expect(padNumber(0, 10)).toBe("01");
		expect(padNumber(99, 100)).toBe("100");
		expect(padNumber(0, 0)).toBe("1");
	});

	it("formats the UTC date", () => {
		expect(utcDate(new Date("2026-11-04T23:59:00Z"))).toBe("2026-11-04");
		expect(utcDate(new Date("2026-11-04T23:59:00-05:00"))).toBe("2026-11-05");
	});

	it("takes the parent from a path, or the fallback at the top", () => {
		expect(parentName("Inbox/Docs/Talk.mp4")).toBe("Docs");
		expect(parentName("/media/Inbox/Film.mp4")).toBe("Inbox");
		expect(parentName("Film.mp4", "Inbox")).toBe("Inbox");
		expect(parentName("Film.mp4")).toBe("");
	});

	it("joins the languages of the kept audio streams", () => {
		const streams = [
			{ kept: true, isAudio: false, language: "jpn" },
			{ kept: true, isAudio: true, language: "ENG" },
			{ kept: false, isAudio: true, language: "fre" },
			{ kept: true, isAudio: true, language: "und" },
			{ kept: true, isAudio: true, language: "ger" },
			{ kept: true, isAudio: true, language: "eng" }
		];
		expect(languagesOf(streams)).toBe("eng-ger");
		expect(languagesOf([{ kept: true, isAudio: true, language: "" }])).toBe("und");
		expect(languagesOf([])).toBe("und");
	});
});

describe("previewNames", () => {
	const files = [
		{ name: "Film 1.mp4", path: "Inbox/Film 1.mp4" },
		{ name: "Film 2.mp4", path: "Inbox/Film 2.mp4" },
		{ name: "Talk.mp4", path: "Inbox/Docs/Talk.mp4" }
	];

	it("renders the first files with their number, parent and date", () => {
		expect(previewNames("{n}-{parent}-{name}.{date}.mkv", files, { date: "2026-11-04" })).toEqual([
			{ input: "Film 1.mp4", output: "1-Inbox-Film 1.2026-11-04.mkv" },
			{ input: "Film 2.mp4", output: "2-Inbox-Film 2.2026-11-04.mkv" },
			{ input: "Talk.mp4", output: "3-Docs-Talk.2026-11-04.mkv" }
		]);
	});

	it("keeps each file's own extension with {ext} and limits the lines", () => {
		expect(previewNames("{name}.{ext}", files, {}, 2)).toEqual([
			{ input: "Film 1.mp4", output: "Film 1.mp4" },
			{ input: "Film 2.mp4", output: "Film 2.mp4" }
		]);
	});

	it("pads the number to the whole batch, not the shown lines", () => {
		const many = Array.from({ length: 12 }, (_, i) => ({ name: `f${i}.mp4`, path: `f${i}.mp4` }));
		expect(previewNames("{n}.mkv", many, {}, 1)).toEqual([{ input: "f0.mp4", output: "01.mkv" }]);
	});

	it("uses the folder name as the parent of a file at the top, and und as the language", () => {
		expect(previewFields({ name: "a.mkv", path: "a.mkv" }, 0, 1, { folderName: "Inbox", date: "2026-01-01" })).toEqual({
			name: "a",
			ext: "mkv",
			lang: "und",
			date: "2026-01-01",
			n: "1",
			parent: "Inbox"
		});
	});

	it("dates the names today by default", () => {
		expect(previewFields({ name: "a.mkv", path: "a.mkv" }, 0, 1).date).toBe(utcDate(new Date()));
	});
});

describe("templateTarget", () => {
	it("names the container the extension picks", () => {
		expect(templateTarget("{name}.mkv")).toBe("MKV");
		expect(templateTarget("{name}.{lang}.MP4")).toBe("MP4");
		expect(templateTarget("{name}.mov")).toBe("MOV");
		expect(templateTarget("{name}.webm")).toBe("WebM");
		expect(templateTarget("{name}.m4a")).toBe("MP4");
	});

	it("says the input keeps its own container with {ext}", () => {
		expect(templateTarget("{name}.{ext}")).toBe("input");
	});

	it("knows no container for a missing or unknown extension", () => {
		expect(templateTarget("{name}")).toBeUndefined();
		expect(templateTarget("{name}.avi")).toBeUndefined();
		expect(templateTarget("{name}.{bad}")).toBeUndefined();
		expect(containerOf("a.ts")).toBeUndefined();
	});
});
