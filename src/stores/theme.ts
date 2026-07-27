import { ref } from "vue";
import { logger } from "../utils/logger";

export type Theme = "dim" | "light";

function systemPrefers(): Theme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dim" : "light";
}

// 每次启动跟随系统
const theme = ref<Theme>(systemPrefers());
// 用户手动切换后，不再跟随系统主题变化
let userOverride = false;

// 系统主题变化时自动跟随（仅当用户未手动切换时）
const media = window.matchMedia("(prefers-color-scheme: dark)");
media.addEventListener("change", (e) => {
  if (userOverride) return;
  theme.value = e.matches ? "dim" : "light";
  logger.action("Theme", "系统主题变化已跟随", { theme: theme.value });
});

export function useTheme() {
  function toggle() {
    theme.value = theme.value === "dim" ? "light" : "dim";
    userOverride = true;
    logger.action("Theme", "手动切换", { theme: theme.value });
  }

  function set(t: Theme) {
    theme.value = t;
    userOverride = true;
  }

  /** 重置为跟随系统主题 */
  function resetToSystem() {
    userOverride = false;
    theme.value = systemPrefers();
    logger.action("Theme", "重置为跟随系统", { theme: theme.value });
  }

  return { theme, toggle, set, resetToSystem };
}
