// 从 SVG 源生成 Tauri 需要的全部图标（PNG / ICO / ICNS）+ 托盘图标。
// 依赖：@resvg/resvg-js（devDependencies）
// 用法：node scripts/make-icons.mjs
// 源文件：
//   scripts/app-icon.svg  —— 桌面图标（蓝渐变 + 折线）
//   scripts/tray-icon.svg —— 状态栏模板图（单色折线，透明底）
import fs from "node:fs";
import path from "node:path";
import { Resvg } from "@resvg/resvg-js";

const ROOT = path.resolve(import.meta.dirname, "..");
const OUT = path.join(ROOT, "src-tauri", "icons");
const TRAY_OUT = path.join(OUT, "tray");
fs.mkdirSync(OUT, { recursive: true });
fs.mkdirSync(TRAY_OUT, { recursive: true });

const appSvg = fs.readFileSync(path.join(import.meta.dirname, "app-icon.svg"), "utf8");
const traySvg = fs.readFileSync(path.join(import.meta.dirname, "tray-icon.svg"), "utf8");

function render(svg, size) {
  const resvg = new Resvg(svg, {
    fitTo: { mode: "width", value: size },
    background: "rgba(0,0,0,0)",
  });
  return resvg.render().asPng();
}

// ---- 桌面图标：Tauri 需要的各尺寸 PNG ----
const p32 = render(appSvg, 32);
const p128 = render(appSvg, 128);
const p256 = render(appSvg, 256);
const p512 = render(appSvg, 512);
const p1024 = render(appSvg, 1024);
fs.writeFileSync(path.join(OUT, "32x32.png"), p32);
fs.writeFileSync(path.join(OUT, "128x128.png"), p128);
fs.writeFileSync(path.join(OUT, "128x128@2x.png"), p256);
fs.writeFileSync(path.join(OUT, "icon.png"), p512);

// ---- ICO（内部直接塞 PNG，Windows 用）----
function buildICO(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type=icon
  header.writeUInt16LE(images.length, 4);
  const entries = [];
  const dataChunks = [];
  let offset = 6 + images.length * 16;
  for (const [size, png] of images) {
    const entry = Buffer.alloc(16);
    entry.writeUInt8(size >= 256 ? 0 : size, 0); // width (0 => 256)
    entry.writeUInt8(size >= 256 ? 0 : size, 1); // height
    entry.writeUInt8(0, 2); // colors
    entry.writeUInt8(0, 3); // reserved
    entry.writeUInt16LE(1, 4); // planes
    entry.writeUInt16LE(32, 6); // bit count
    entry.writeUInt32LE(png.length, 8); // bytes
    entry.writeUInt32LE(offset, 12); // offset
    entries.push(entry);
    dataChunks.push(png);
    offset += png.length;
  }
  return Buffer.concat([header, ...entries, ...dataChunks]);
}
fs.writeFileSync(path.join(OUT, "icon.ico"), buildICO([[32, p32], [256, p256]]));

// ---- ICNS（macOS 用；ic07=128, ic08=256, ic09=512, ic10=1024）----
function icnsEntry(type, png) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(png.length + 8, 0);
  return Buffer.concat([Buffer.from(type, "ascii"), len, png]);
}
const icnsBody = Buffer.concat([
  icnsEntry("ic07", p128),
  icnsEntry("ic08", p256),
  icnsEntry("ic09", p512),
  icnsEntry("ic10", p1024),
]);
const icnsHeader = Buffer.alloc(8);
icnsHeader.write("icns", 0, "ascii");
icnsHeader.writeUInt32BE(icnsBody.length + 8, 4);
fs.writeFileSync(path.join(OUT, "icon.icns"), Buffer.concat([icnsHeader, icnsBody]));

// ---- 托盘模板图：macOS 菜单栏用 @1x(22) + @2x(44)；Windows 用 32 ----
fs.writeFileSync(path.join(TRAY_OUT, "tray-22.png"), render(traySvg, 22));
fs.writeFileSync(path.join(TRAY_OUT, "tray-22@2x.png"), render(traySvg, 44));
fs.writeFileSync(path.join(TRAY_OUT, "tray-32.png"), render(traySvg, 32));

console.log("icons generated in", OUT);
console.log("  app :", ["32x32.png", "128x128.png", "128x128@2x.png", "icon.png", "icon.ico", "icon.icns"].join(", "));
console.log("  tray:", "tray/tray-22.png, tray/tray-22@2x.png, tray/tray-32.png");
