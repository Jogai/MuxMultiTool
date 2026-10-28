import { create } from "@bufbuild/protobuf";
import { Code, type Client, ConnectError } from "@connectrpc/connect";
import { ref } from "vue";
import { describe, expect, it, vi } from "vitest";

import { type ThemeStorage, useThemeSettings } from "./useThemeSettings";
import { THEME_STORAGE_KEY, type ThemeRoot, type ThemeSetting } from "~/core/libs/theme";
import {
	GetSettingsResponseSchema,
	type SystemService,
	ThemeMode,
	UpdateSettingsResponseSchema
} from "~/generated/muxmultitool/system/v1/system_pb";

const catppuccinAuto: ThemeSetting = { family: "catppuccin", mode: "auto", variant: "mocha", lightVariant: "latte", darkVariant: "mocha" };

function fakeClient(overrides: Partial<Client<typeof SystemService>> = {}): Client<typeof SystemService> {
	return {
		getHealth: vi.fn(),
		getVersion: vi.fn(),
		getCapabilities: vi.fn(),
		getSettings: vi.fn().mockResolvedValue(create(GetSettingsResponseSchema, { settings: {} })),
		updateSettings: vi.fn().mockImplementation((request) => Promise.resolve(create(UpdateSettingsResponseSchema, request))),
		...overrides
	} as unknown as Client<typeof SystemService>;
}

function fakeStorage(initial?: object): ThemeStorage & { map: Map<string, string> } {
	const map = new Map<string, string>();
	if (initial) {
		map.set(THEME_STORAGE_KEY, JSON.stringify(initial));
	}
	return { map, getItem: (key) => map.get(key) ?? null, setItem: (key, value) => void map.set(key, value) };
}

function fakeRoot(): ThemeRoot & { properties: Map<string, string> } {
	const properties = new Map<string, string>();
	return { properties, style: { setProperty: (name, value) => properties.set(name, value) }, dataset: {} };
}

describe("useThemeSettings", () => {
	it("starts from the browser copy and applies it to the root before the server answers", () => {
		const root = fakeRoot();
		const theme = useThemeSettings({ client: fakeClient(), prefersDark: ref(true), storage: fakeStorage(catppuccinAuto), root });

		expect(theme.setting.value).toEqual(catppuccinAuto);
		expect(theme.resolved.value.variant.id).toBe("mocha");
		expect(root.properties.get("--mmt-surface")).toBe("#1e1e2e");
		expect(root.dataset["theme"]).toBe("catppuccin/mocha");
	});

	it("defaults to the default palette in auto mode without a browser copy", () => {
		const theme = useThemeSettings({ client: fakeClient(), prefersDark: ref(false), storage: fakeStorage() });
		expect(theme.setting.value.family).toBe("default");
		expect(theme.resolved.value.variant.id).toBe("light");
	});

	it("takes the server's theme over the browser copy and mirrors it", async () => {
		const storage = fakeStorage(catppuccinAuto);
		const client = fakeClient({
			getSettings: vi.fn().mockResolvedValue(
				create(GetSettingsResponseSchema, {
					settings: { theme: { family: "nord", mode: ThemeMode.SINGLE, variant: "nord" } }
				})
			)
		});
		const theme = useThemeSettings({ client, prefersDark: ref(true), storage });

		await vi.waitFor(() => expect(theme.loaded.value).toBe(true));
		expect(theme.setting.value.family).toBe("nord");
		expect(theme.setting.value.mode).toBe("single");
		expect(JSON.parse(storage.map.get(THEME_STORAGE_KEY) ?? "{}")).toMatchObject({ family: "nord" });
	});

	it("keeps the browser copy when the server has no theme or cannot be reached", async () => {
		const noTheme = useThemeSettings({ client: fakeClient(), prefersDark: ref(true), storage: fakeStorage(catppuccinAuto) });
		await vi.waitFor(() => expect(noTheme.loaded.value).toBe(true));
		expect(noTheme.setting.value).toEqual(catppuccinAuto);

		const client = fakeClient({ getSettings: vi.fn().mockRejectedValue(new ConnectError("down", Code.Unavailable)) });
		const offline = useThemeSettings({ client, prefersDark: ref(true), storage: fakeStorage(catppuccinAuto) });
		await vi.waitFor(() => expect(offline.loaded.value).toBe(true));
		expect(offline.setting.value).toEqual(catppuccinAuto);
		expect(offline.saveError.value).toBeUndefined();
	});

	it("follows the OS in auto mode as it changes", () => {
		const prefersDark = ref(false);
		const root = fakeRoot();
		useThemeSettings({ client: fakeClient(), prefersDark, storage: fakeStorage(catppuccinAuto), root });
		expect(root.dataset["theme"]).toBe("catppuccin/latte");
		expect(root.properties.get("color-scheme")).toBe("light");

		prefersDark.value = true;
		return vi.waitFor(() => {
			expect(root.dataset["theme"]).toBe("catppuccin/mocha");
			expect(root.properties.get("color-scheme")).toBe("dark");
		});
	});

	it("saves an update on the server and in the browser, resetting what no longer fits", async () => {
		const client = fakeClient();
		const storage = fakeStorage(catppuccinAuto);
		const theme = useThemeSettings({ client, prefersDark: ref(true), storage });
		await vi.waitFor(() => expect(theme.loaded.value).toBe(true));

		theme.update({ family: "dracula" });
		expect(theme.setting.value).toEqual({
			family: "dracula",
			mode: "auto",
			variant: "dracula",
			lightVariant: "alucard",
			darkVariant: "dracula"
		});
		expect(theme.resolved.value.variant.id).toBe("dracula");
		expect(JSON.parse(storage.map.get(THEME_STORAGE_KEY) ?? "{}")).toEqual(theme.setting.value);
		await vi.waitFor(() => expect(client.updateSettings).toHaveBeenCalledTimes(1));
		expect(client.updateSettings).toHaveBeenCalledWith({
			settings: {
				theme: expect.objectContaining({ family: "dracula", mode: ThemeMode.AUTO, lightVariant: "alucard", darkVariant: "dracula" })
			}
		});
	});

	it("reports a failed save and keeps the choice locally", async () => {
		const client = fakeClient({ updateSettings: vi.fn().mockRejectedValue(new ConnectError("disk full", Code.Internal)) });
		const storage = fakeStorage();
		const theme = useThemeSettings({ client, prefersDark: ref(true), storage });

		theme.update({ family: "nord" });
		await vi.waitFor(() => expect(theme.saveError.value).toBeDefined());
		expect(theme.saveError.value).toMatchObject({ codeName: "internal", message: "disk full" });
		expect(theme.setting.value.family).toBe("nord");
		expect(storage.map.get(THEME_STORAGE_KEY)).toContain('"nord"');
	});

	it("works without storage or a root", async () => {
		const theme = useThemeSettings({ client: fakeClient(), prefersDark: ref(true), storage: undefined, root: undefined });
		await vi.waitFor(() => expect(theme.loaded.value).toBe(true));
		theme.update({ family: "one" });
		expect(theme.resolved.value.variant.id).toBe("dark");
	});
});
