// 前端 IPv4 校验，**逐条对齐** `gateshift-core/src/util.rs` 的 `is_ipv4`。
//
// 前端校验只是为了「用户输完就能看到提示」，真正把关的是 core（非法输入在拼命令之前
// 就被拒掉，绝不碰系统）。所以两边规则必须一致：前端放过的值，保存时被 core 拒掉，
// 用户看到的是没法理解的后端报错。
//
// core 明确拒绝的写法（前端以前是放过的，尤其是前导零）：
//   - 段数不是 4            "192.168.1"
//   - 段为空 / 超过 3 位     "192.168..1"、"192.168.0001.1"
//   - 段里有非 ASCII 数字    "192.168.1.-1"、"1e2"、全角数字
//   - 前导零                "192.168.01.1"（某些解析器会当成八进制 1）
//   - 段大于 255            "192.168.1.256"
export function isIpv4(s: string): boolean {
  const parts = s.split(".");
  if (parts.length !== 4) return false;
  return parts.every((part) => {
    if (part.length === 0 || part.length > 3) return false;
    if (!/^[0-9]+$/.test(part)) return false;
    if (part.length > 1 && part.startsWith("0")) return false;
    return Number(part) <= 255;
  });
}
