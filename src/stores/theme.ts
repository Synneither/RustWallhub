import { ref } from "vue";
import { logger } from "../utils/logger";

export type Theme = "dim" | "light";

function systemPrefers(): Theme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dim" : "light";
}

// 每次启动跟随系统
const theme = ref<Theme>(systemPrefers());

// 系统主题变化时自动跟随
const media = window.matchMedia("(prefers-color-scheme: dark)");
media.addEventListener("change", (e) => {
  theme.value = e.matches ? "dim" : "light";
  logger.action("Theme", "系统主题变化已跟随", { theme: theme.value });
});

export function useTheme() {
  function toggle() {
    theme.value = theme.value === "dim" ? "light" : "dim";
    logger.action("Theme", "手动切换", { theme: theme.value });
  }

  function set(t: Theme) {
    theme.value = t;
  }

  return { theme, toggle, set };
}
