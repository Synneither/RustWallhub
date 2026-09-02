import { ref } from "vue";
import { logger } from "../utils/logger";

export type Theme = "dim" | "light";

const STORAGE_KEY = "rustwallhub-theme";

function systemPrefers(): Theme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dim" : "light";
}

function readStored(): Theme | null {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return v === "dim" || v === "light" ? v : null;
  } catch {
    return null;
  }
}

// 启动时优先读用户上次的手动选择；未选择过则跟随系统
const stored = readStored();
const theme = ref<Theme>(stored ?? systemPrefers());
/** 用户是否手动指定过主题（false = 跟随系统） */
const userOverride = ref(stored !== null);

// 系统主题变化时自动跟随（仅当用户未手动指定时）
const media = window.matchMedia("(prefers-color-scheme: dark)");
const onMediaChange = (e: MediaQueryListEvent) => {
  if (userOverride.value) return;
  theme.value = e.matches ? "dim" : "light";
  logger.action("Theme", "系统主题变化已跟随", { theme: theme.value });
};
media.addEventListener("change", onMediaChange);

// HMR 时移除旧监听器，避免热更新后监听器叠加
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    media.removeEventListener("change", onMediaChange);
  });
}

function persist() {
  try {
    if (userOverride.value) localStorage.setItem(STORAGE_KEY, theme.value);
    else localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* localStorage 不可用时静默降级为会话内生效 */
  }
}

export function useTheme() {
  function toggle() {
    theme.value = theme.value === "dim" ? "light" : "dim";
    userOverride.value = true;
    persist();
    logger.action("Theme", "手动切换", { theme: theme.value });
  }

  function set(t: Theme) {
    theme.value = t;
    userOverride.value = true;
    persist();
  }

  /** 重置为跟随系统主题 */
  function resetToSystem() {
    userOverride.value = false;
    theme.value = systemPrefers();
    persist();
    logger.action("Theme", "重置为跟随系统", { theme: theme.value });
  }

  return { theme, userOverride, toggle, set, resetToSystem };
}
