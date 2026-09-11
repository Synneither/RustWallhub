import { ref, type Ref } from "vue";
import { toast, toastError } from "../stores/app";
import { friendlyError } from "../utils/errors";

/**
 * 把「异步按钮」的样板收敛成一个 composable。
 *
 * 项目里有十几处这样的结构：
 *   loading.value = true;
 *   try { await doSomething(a, b); }
 *   catch (e) { toastError(e); }
 *   finally { loading.value = false; }
 *
 * 用它替换后：
 *   const { run, loading } = useAsyncAction(doSomething);
 *   <v-btn :loading="loading" @click="run" />
 *
 * 额外保证：
 * - 进行中重复点击会被忽略（`busy` 是同步检查，不受响应式更新时机影响）
 * - 可传入 onSuccess/onError 做额外处理，错误仍然会被统一 toast
 */
export interface AsyncActionOptions {
  /** 执行成功后的回调（不会因 hook 抛错而吞掉原始结果） */
  onSuccess?: (result: unknown) => void | Promise<void>;
  /** 自定义错误处理；不提供时走 toastError */
  onError?: (e: unknown) => void;
  /** 错误提示文案前缀，便于区分是哪个操作失败 */
  errorPrefix?: string;
}

export function useAsyncAction<A extends unknown[], T>(
  fn: (...args: A) => Promise<T>,
  options: AsyncActionOptions = {},
): { run: (...args: A) => Promise<T | undefined>; loading: Ref<boolean> } {
  const loading = ref(false);
  // 同步的忙碌标志：loading 是 ref，连续两次点击在同一 tick 内可能都读到 false。
  let busy = false;

  async function run(...args: A): Promise<T | undefined> {
    if (busy) return undefined;
    busy = true;
    loading.value = true;
    try {
      const result = await fn(...args);
      if (options.onSuccess) await options.onSuccess(result);
      return result;
    } catch (e) {
      if (options.onError) {
        options.onError(e);
      } else if (options.errorPrefix) {
        toast(`${options.errorPrefix}：${friendlyError(e)}`, "error");
      } else {
        toastError(e);
      }
      return undefined;
    } finally {
      busy = false;
      loading.value = false;
    }
  }

  return { run, loading };
}
