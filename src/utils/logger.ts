type LogLevel = "INFO" | "WARN" | "ERROR" | "ACTION";

function timestamp(): string {
  return new Date().toISOString().replace("T", " ").slice(0, 19);
}

function log(level: LogLevel, context: string, message: string, data?: unknown) {
  const prefix = `[${timestamp()}] [${level}] [${context}]`;
  if (data !== undefined) {
    const extra = typeof data === "string" ? data : JSON.stringify(data);
    console.log(`${prefix} ${message} | ${extra}`);
  } else {
    console.log(`${prefix} ${message}`);
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
