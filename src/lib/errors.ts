// IPC 错误的结构化处理。
//
// 后端 Tauri 命令统一把错误序列化成 `{ code, message }`（见 `src-tauri/src/commands.rs`
// 与 `gateshift-core` 的 `Error::code()`）：`code` 是稳定的机器可读标识，`message` 是
// 给人看的中文文案。
//
// **分支判断只允许用 `code`**。之前用 `message.includes("取消")` 判断用户是否取消授权，
// 文案一改（「已取消系统授权」→「授权被取消」）行为就悄悄变了，而且任何含「取消」二字的
// 报错都会被误判成「已取消」。

/** 后端命令错误载荷（与 `src-tauri/src/commands.rs::CommandError` 对齐）。 */
export interface CommandError {
  code: string;
  message: string;
}

/** 用户取消了系统授权（Touch ID / 密码框 / UAC 被拒）。core 的 `Error::ElevationCancelled`。 */
export const CODE_ELEVATION_CANCELLED = "elevation_cancelled";

/** 命令层自身的问题（阻塞线程池 join 失败等），不属于 core 的错误域。 */
export const CODE_INTERNAL = "internal";

/** 前端兜底：插件抛的普通 `Error`、字符串、或任何没有 `code` 的异常。 */
export const CODE_UNKNOWN = "unknown";

/**
 * 把 `invoke` 抛出的任意异常规整成 `{ code, message }`。
 *
 * Tauri v2 会把命令的 `Err(CommandError)` 原样反序列化成 JS 对象（不是 `Error` 实例），
 * 但开机自启插件（`tauri-plugin-autostart`）抛的是普通 `Error`，所以这里两种都要认。
 */
export function toCommandError(e: unknown): CommandError {
  if (e && typeof e === "object") {
    const { code, message } = e as { code?: unknown; message?: unknown };
    if (typeof code === "string" && code !== "") {
      return { code, message: typeof message === "string" ? message : code };
    }
    if (typeof message === "string") return { code: CODE_UNKNOWN, message };
  }
  if (typeof e === "string" && e.trim() !== "") {
    return { code: CODE_UNKNOWN, message: e };
  }
  return { code: CODE_UNKNOWN, message: "未知错误" };
}

/** 展示用文案（永远有内容，不会出现 `undefined`）。 */
export function errorMessage(e: unknown): string {
  return toCommandError(e).message;
}

/** 用户是否只是取消了授权（这种情况应当记「什么都没改」，而不是报错）。 */
export function isElevationCancelled(e: unknown): boolean {
  return toCommandError(e).code === CODE_ELEVATION_CANCELLED;
}
