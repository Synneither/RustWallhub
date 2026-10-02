import { onBeforeUnmount, reactive } from "vue";

/**
 * 卡片多选状态。两套实现对应两种真实需求：
 *
 * - [`useSelection`]：只关心「选没选」的字符串键（图库按文件名选中）。
 * - [`useSelectionMap`]：选中时要保留整条记录（Wallhaven 按 id 选中，下载选中项要把
 *   每条记录的完整信息发给后端，所以存 Map<id, entry> 而不是 Set<id>）。
 *
 * 都用 `reactive(new Set/Map())` 直接操作，调用方无需 `.value`。
 */

/** 只记键的多选（图库：键是文件名）。 */
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

/** 键 → 记录的多选（Wallhaven：键是图片 id，值要留给下载选中项用）。 */
export function useSelectionMap<K, V>() {
  // reactive() 会把值类型深度解包成 UnwrapRefSimple<V>，与调用方声明的 V 对不上。
  // 存进来的都是搜索结果里的普通对象、不需要深度代理，所以按 V 声明使用。
  const selected = reactive(new Map<K, V>()) as unknown as Map<K, V>;

  function toggle(key: K, value: V) {
    if (selected.has(key)) selected.delete(key);
    else selected.set(key, value);
  }

  /** 整批替换（全选 / 全不选）。 */
  function setAll(entries: Iterable<readonly [K, V]>) {
    selected.clear();
    for (const [key, value] of entries) selected.set(key, value);
  }

  function clear() {
    selected.clear();
  }

  return { selected, toggle, setAll, clear };
}

/**
 * 同一元素上「单击做一件事、双击做另一件事」的消歧。
 *
 * 直接把 `click` 和 `dblclick` 绑在同一元素上，双击会先触发两次 `click`（选中又取消、
 * 界面闪烁），所以这里用延迟判定：单击要等 `delayMs` 内没人再点才算数。
 *
 * **必须记住上一次点击对应的整条目，不能只记 id**：在延迟窗口内从 A 换到 B 时，
 * 如果只比 id，就会把「A、B 各点一下」误判成「对 B 的双击」而直接弹预览，
 * 用户想连选两张时就会误触。
 */
export function useClickOrDoubleClick<T extends { id: string }>(options: {
  onSingle: (item: T) => void;
  onDouble: (item: T) => void;
  delayMs?: number;
}) {
  const delay = options.delayMs ?? 250;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending: T | null = null;

  function cancel() {
    if (timer) clearTimeout(timer);
    timer = null;
    pending = null;
  }

  function handle(item: T) {
    if (timer) {
      clearTimeout(timer);
      timer = null;
      const prev = pending;
      pending = null;
      if (prev?.id === item.id) {
        // 同一张连续两次点击 = 双击
        options.onDouble(item);
        return;
      }
      // 换了一张卡片：上一张按单击结算，避免这次点击既丢失选择又误开预览
      if (prev) options.onSingle(prev);
    }
    pending = item;
    timer = setTimeout(() => {
      timer = null;
      pending = null;
      options.onSingle(item);
    }, delay);
  }

  // 组件销毁时残留的计时器没有意义，而且会持有已失效的条目引用。
  onBeforeUnmount(cancel);

  return { handle, cancel };
}
