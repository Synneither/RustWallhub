import { computed, shallowRef, type ComputedRef, type ShallowRef } from "vue";

/**
 * 缩略图 URL 缓存（文件名 → asset URL），LRU 淘汰 + 容量上限。
 *
 * GalleryView 与 NewImagesStrip 此前各有一份逐字相同的实现（仅上限不同：
 * 600 / 200），这里收敛成单一实现，新增消费方直接复用。
 *
 * 原实现里的两个关键约定保持不变：
 * - shallowRef<Map> + 整批替换引用：一次写入只触发一次响应式更新。
 *   此前用 reactive<Record<string, string>> 时 600 个键会被逐键深度代理，
 *   切来源时逐个 delete 触发 600 次独立更新。
 * - get() 只读不写：它在渲染期间被模板调用，若内部改 Map 会触发渲染中的
 *   响应式更新（Vue 会告警，严重时死循环）。LRU 位置只在写入时更新。
 */
export interface ThumbCache {
  /** 缓存本体；读用 get()，不要在模板里直接遍历 */
  urls: ShallowRef<Map<string, string>>;
  /** 当前缓存条数（响应式） */
  size: ComputedRef<number>;
  /** 只读查询，不更新 LRU 位置 */
  get(name: string): string | undefined;
  /** 批量写入：克隆一次、整体替换引用，只触发一次更新 */
  cache(entries: Iterable<readonly [string, string]>): void;
  /** 整批丢弃（切来源 / 缩略图档位变化时调用） */
  clear(): void;
}

export function useThumbCache(max: number): ThumbCache {
  const urls = shallowRef<Map<string, string>>(new Map());
  const size = computed(() => urls.value.size);

  function get(name: string): string | undefined {
    return urls.value.get(name);
  }

  function cache(entries: Iterable<readonly [string, string]>): void {
    const next = new Map(urls.value);
    for (const [name, url] of entries) {
      // 已存在的键先删再写，挪到 Map 末尾（最近使用），
      // 淘汰顺序就是 LRU 而不是插入顺序——翻回旧页时不会重复取缩略图。
      next.delete(name);
      next.set(name, url);
    }
    // Map 保持插入顺序，超限时从最久未使用的条目开始丢弃
    while (next.size > max) {
      const oldest = next.keys().next().value;
      if (oldest === undefined) break;
      next.delete(oldest);
    }
    urls.value = next;
  }

  function clear(): void {
    urls.value = new Map();
  }

  return { urls, size, get, cache, clear };
}
