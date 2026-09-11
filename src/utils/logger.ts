type LogLevel = "INFO" | "WARN" | "ERROR" | "ACTION";

function timestamp(): string {
  return new Date().toISOString().replace("T", " ").slice(0, 19);
}

function safeStringify(data: unknown): string {
  if (data === null) return "null";
  if (data === undefined) return "undefined";
  if (typeof data === "string") return data;
  try {
    return JSON.stringify(data);
  } catch {
    // Circular reference or other serialization error
    return String(data);
  }
}

function log(level: LogLevel, context: string, message: string, data?: unknown) {
  // INFO / ACTION 当前不输出，提前 return：否则每次调用都要白做一次时间戳构造
  // 和 JSON.stringify（大数据对象上是可观的开销）。这里保持与 switch 的默认分支一致。
  if (level !== "ERROR" && level !== "WARN") return;

  const prefix = `[${timestamp()}] [${level}] [${context}]`;
  const logMsg =
    data !== undefined
      ? `${prefix} ${message} | ${safeStringify(data)}`
      : `${prefix} ${message}`;

  switch (level) {
    case "ERROR":
      console.error(logMsg);
      break;
    case "WARN":
      console.warn(logMsg);
      break;
    default:
      break;
  }
}

export const logger = {
  info(context: string, message: string, data?: unknown) {
    log("INFO", context, message, data);
  },
  warn(context: string, message: string, data?: unknown) {
    log("WARN", context, message, data);
  },
  error(context: string, message: string, data?: unknown) {
    log("ERROR", context, message, data);
  },
  action(context: string, message: string, data?: unknown) {
    log("ACTION", context, message, data);
  },
};
