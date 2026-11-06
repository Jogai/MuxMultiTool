import { createPinia } from "pinia";
import { createApp } from "vue";
import { createI18n } from "vue-i18n";
import { createRouter, createWebHistory } from "vue-router";
import { routes } from "vue-router/auto-routes";

import messages from "@intlify/unplugin-vue-i18n/messages";

import App from "./App.vue";
import "./core/themes/base.css";

const router = createRouter({
	history: createWebHistory(),
	routes
});

const i18n = createI18n({
	legacy: false,
	locale: "en",
	fallbackLocale: "en",
	messages
});

createApp(App).use(createPinia()).use(router).use(i18n).mount("#app");
