import type { Client } from "@connectrpc/connect";
import { usePreferredDark } from "@vueuse/core";
import { computed, type Ref, ref, watchEffect } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import {
	applyThemeTokens,
	normalizeThemeSetting,
	parseStoredTheme,
	resolveTheme,
	THEME_STORAGE_KEY,
	type ThemeRoot,
	type ThemeSetting,
	themeSettingFromProto,
	themeSettingToProto
} from "~/core/libs/theme";
import { resolveTokens } from "~/core/themes/palette";
import { SystemService } from "~/generated/muxmultitool/system/v1/system_pb";

/** The bits of `localStorage` the theme uses; tests pass a map. */
export type ThemeStorage = Pick<Storage, "getItem" | "setItem">;

export interface ThemeSettingsOptions {
	client?: Client<typeof SystemService>;
	prefersDark?: Ref<boolean>;
	/** Where the browser copy lives; `undefined` when storage is unavailable (private mode, tests). */
	storage?: ThemeStorage;
	/** Where the tokens are written; `undefined` skips the DOM (tests). */
	root?: ThemeRoot;
}

function browserStorage(): ThemeStorage | undefined {
	try {
		return typeof localStorage === "undefined" ? undefined : localStorage;
	} catch {
		return undefined;
	}
}

function browserRoot(): ThemeRoot | undefined {
	return typeof document === "undefined" ? undefined : document.documentElement;
}

/**
 * The theme choice: applied to the page, kept on the server through the system service, and mirrored in
 * the browser so a reload shows the right theme before the server answers (and without a server at all).
 * The server copy wins when it exists, so every tab and the desktop window agree.
 */
export function useThemeSettings({
	client = useRpcClient(SystemService),
	prefersDark = usePreferredDark(),
	storage = browserStorage(),
	root = browserRoot()
}: ThemeSettingsOptions = {}) {
	const setting = ref<ThemeSetting>(normalizeThemeSetting(parseStoredTheme(storage?.getItem(THEME_STORAGE_KEY))));
	/** Whether the server has been asked yet, however it answered. */
	const loaded = ref(false);
	/** Set when the last save on the server failed; the browser copy still holds the choice. */
	const saveError = ref<AppError>();

	const resolved = computed(() => resolveTheme(setting.value, prefersDark.value));
	const tokens = computed(() => resolveTokens(resolved.value.variant));

	if (root) {
		watchEffect(() => applyThemeTokens(root, resolved.value, tokens.value));
	}

	function remember(): void {
		try {
			storage?.setItem(THEME_STORAGE_KEY, JSON.stringify(setting.value));
		} catch {
			// Storage full or blocked: the server copy and this session still have the choice.
		}
	}

	async function load(): Promise<void> {
		try {
			const response = await client.getSettings({});
			if (response.settings?.theme) {
				setting.value = normalizeThemeSetting(themeSettingFromProto(response.settings.theme));
				remember();
			}
		} catch {
			// No backend, or none yet: the browser copy stays in charge.
		} finally {
			loaded.value = true;
		}
	}

	async function save(): Promise<void> {
		try {
			await client.updateSettings({ settings: { theme: themeSettingToProto(setting.value) } });
			saveError.value = undefined;
		} catch (cause) {
			saveError.value = toAppError(cause);
		}
	}

	/** Changes part of the setting; the rest is kept, and anything that no longer fits the family is reset. */
	function update(patch: Partial<ThemeSetting>): void {
		setting.value = normalizeThemeSetting({ ...setting.value, ...patch });
		remember();
		void save();
	}

	void load();

	return { setting, resolved, tokens, loaded, saveError, update, load, save };
}
