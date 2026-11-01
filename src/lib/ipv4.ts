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

// 前端 IPv6 校验，**逐条对齐** `gateshift-core/src/util.rs` 的 `is_ipv6`。
//
// 与 isIpv4 同理：前端校验只为即时提示，真正把关的是 core。规则必须一致：
//   - 8 组十六进制的完整形式，或含一次 `::` 的压缩形式
//   - `::` 只能出现一次          "::1::2" 非法
//   - 不能有多余冒号              "2400:3200:::1" 非法
//   - 组数上限 8                  "1:2:3:4:5:6:7:8:9" 非法
//   - 单组 1-4 位十六进制         "12345::" 非法；"0db8" 合法（组内前导零允许）
//   - IPv4 映射形式（::ffff:1.2.3.4）不支持
export function isIpv6(s: string): boolean {
  if (!s) return false;
  // `::` 只能出现一次；出现即压缩形式（省略了一串全零组）
  const sep = s.indexOf("::");
  let head: string, tail: string | null;
  if (sep !== -1) {
    head = s.slice(0, sep);
    tail = s.slice(sep + 2);
    if (tail.includes("::")) return false;
  } else {
    head = s;
    tail = null;
  }

  // 数一段里用 `:` 分隔的合法组数；任何一组非法就整体不合法
  const countGroups = (part: string): number | null => {
    if (part === "") return 0;
    let n = 0;
    for (const group of part.split(":")) {
      n++;
      // 每组 1-4 位十六进制；空组 / 超 4 位都非法
      if (group.length === 0 || group.length > 4) return null;
      if (!/^[0-9a-fA-F]+$/.test(group)) return null;
    }
    return n;
  };

  const headGroups = countGroups(head);
  if (headGroups === null) return false;
  if (tail === null) {
    // 完整形式：必须恰好 8 组
    return headGroups === 8;
  }
  const tailGroups = countGroups(tail);
  if (tailGroups === null) return false;
  // 压缩形式：`::` 至少省略一组，所以非零组总数最多 7
  return headGroups + tailGroups <= 7;
}
