import { reactive } from "vue";

/**
 * 图片卡片多选状态（GalleryView / WallhavenView 共用）。
 * 两个视图此前各自声明 `reactive(new Set<string>())` + 逐字重复的 `toggleSelect`，
 * 这里收敛成单一实现；`reactive(new Set())` 直接操作，无需 `.value`。
 */
export function useSelection() {
  const selected = reactive(new Set<string>());

  function toggle(key: string) {
    if (selected.has(key)) selected.delete(key);
    else selected.add(key);
  }

  function clear() {
    selected.clear();
  }

  return { selected, toggle, clear };
}
