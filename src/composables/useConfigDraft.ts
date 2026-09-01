import { onMounted, reactive, ref } from "vue";
import type { AppConfig } from "../types";
import { appState, toastError } from "../stores/app";
import { saveSettings } from "../utils/api";

/**
 * 配置草稿：把「reactive 草稿 + onMounted 从 appState.config 拷贝 + isDirty + persist」
 * 这一套在 WallhavenView / RedditView / SettingsView 重复的逻辑收敛到一处。
 *
 * @param keys    需要纳入草稿的配置字段（决定 dirty 比较与保存时的合并范围）
 * @param defaults 草稿初始值（后端配置尚未加载时的占位，之后 onMounted 会用真实值覆盖）
 */
export function useConfigDraft<K extends keyof AppConfig>(
  keys: readonly K[],
  defaults: Pick<AppConfig, K>,
) {
  const draft = reactive<Pick<AppConfig, K>>({ ...defaults });
  const saving = ref(false);

  onMounted(() => {
    const c = appState.config;
    if (!c) return;
    for (const key of keys) {
      // K 是联合类型时，`draft[key] = c[key]` 会触发 TS 的 correlated-union 限制，
      // 这里放宽一次索引类型后逐字段拷贝。
      (draft as unknown as Record<string, unknown>)[key as string] = c[key];
    }
  });

  function isDirty(): boolean {
    const c = appState.config;
    if (!c) return false;
    const d = draft as unknown as Record<string, unknown>;
    return keys.some((key) => d[key as string] !== c[key]);
  }

  async function persist(): Promise<boolean> {
    if (!appState.config) return false;
    if (!isDirty()) return true;
    saving.value = true;
    try {
      const next: AppConfig = { ...appState.config, ...draft };
      await saveSettings(next);
      appState.config = next;
      return true;
    } catch (e) {
      toastError(e);
      return false;
    } finally {
      saving.value = false;
    }
  }

  return { draft, isDirty, saving, persist };
}
