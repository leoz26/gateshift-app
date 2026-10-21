// 内联 SVG 图标集（无第三方依赖）。
// 约定：24x24 视框、stroke=currentColor、默认 16px，随字号/父级颜色走。

import type { SVGProps } from "react";

type IconProps = {
  size?: number;
  className?: string;
};

function svgProps({ size = 16, className }: IconProps): SVGProps<SVGSVGElement> {
  return {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    className,
    focusable: false,
    "aria-hidden": true,
  };
}

/** 品牌标记：盾牌 + 双向切换箭头（网关切换） */
export function IconLogo(p: IconProps) {
  return (
    <svg {...svgProps({ size: 20, ...p })}>
      <path d="M12 3l7 3v6c0 4.4-3 7.6-7 9-4-1.4-7-4.6-7-9V6l7-3Z" />
      <path d="M8.5 10.2h5.6" />
      <path d="m12.4 8.3 2 1.9-2 1.9" />
      <path d="M15.5 15.2h-5.6" />
      <path d="m11.6 13.3-2 1.9 2 1.9" />
    </svg>
  );
}

/** 主路由：盾 + 对勾（安全态） */
export function IconShield(p: IconProps) {
  return (
    <svg {...svgProps(p)}>
      <path d="M12 3l7 3v6c0 4.4-3 7.6-7 9-4-1.4-7-4.6-7-9V6l7-3Z" />
      <path d="m8.8 12 2.2 2.2 4.2-4.4" />
    </svg>
  );
}

/** 旁路由：路由路径（切换态） */
export function IconRoute(p: IconProps) {
  return (
    <svg {...svgProps(p)}>
      <circle cx="6" cy="19" r="2.6" />
      <path d="M9 19h8.4a3.4 3.4 0 0 0 0-6.8H6.6a3.4 3.4 0 0 1 0-6.8H15" />
      <circle cx="18" cy="5.4" r="2.6" />
    </svg>
  );
}

/** 刷新 */
export function IconRefresh(p: IconProps) {
  return (
    <svg {...svgProps({ size: 14, ...p })}>
      <path d="M3 12a9 9 0 0 1 9-9 9.7 9.7 0 0 1 6.7 2.7L21 8" />
      <path d="M21 3v5h-5" />
      <path d="M21 12a9 9 0 0 1-9 9 9.7 9.7 0 0 1-6.7-2.7L3 16" />
      <path d="M3 21v-5h5" />
    </svg>
  );
}

/** 网关：两点之间的转发 */
export function IconGateway(p: IconProps) {
  return (
    <svg {...svgProps({ size: 13, ...p })}>
      <circle cx="5" cy="12" r="2.2" />
      <circle cx="19" cy="12" r="2.2" />
      <path d="M7.5 12h9" />
      <path d="m14.3 9.6 2.4 2.4-2.4 2.4" />
    </svg>
  );
}

/** DNS：地球 */
export function IconGlobe(p: IconProps) {
  return (
    <svg {...svgProps({ size: 13, ...p })}>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18" />
      <path d="M12 3a14.5 14.5 0 0 1 0 18 14.5 14.5 0 0 1 0-18Z" />
    </svg>
  );
}

/** 网络接口：网卡 */
export function IconNic(p: IconProps) {
  return (
    <svg {...svgProps({ size: 13, ...p })}>
      <rect x="2.5" y="4" width="19" height="7" rx="2" />
      <rect x="2.5" y="13.5" width="19" height="6.5" rx="2" />
      <path d="M6.4 7.5h.01M6.4 16.8h.01" />
    </svg>
  );
}

/** 撤销 / 还原为自动 */
export function IconUndo(p: IconProps) {
  return (
    <svg {...svgProps(p)}>
      <path d="M3 12a9 9 0 1 0 9-9 9.7 9.7 0 0 0-6.7 2.7L3 8" />
      <path d="M3 3v5h5" />
    </svg>
  );
}

/** 留底 / 存档 */
export function IconArchive(p: IconProps) {
  return (
    <svg {...svgProps(p)}>
      <rect x="2.5" y="3.5" width="19" height="5" rx="1.5" />
      <path d="M4.5 8.5V19a1.5 1.5 0 0 0 1.5 1.5h12a1.5 1.5 0 0 0 1.5-1.5V8.5" />
      <path d="M10 12.5h4" />
    </svg>
  );
}

/** 还原留底 */
export function IconRestore(p: IconProps) {
  return (
    <svg {...svgProps(p)}>
      <path d="M3.5 12a8.5 8.5 0 1 1 2.6 6.1" />
      <path d="M3.5 20.5V15h5.5" />
      <path d="M12 8.5V12l2.6 1.6" />
    </svg>
  );
}

/** 保存 */
export function IconSave(p: IconProps) {
  return (
    <svg {...svgProps(p)}>
      <path d="M5 4h11l3 3v13H5z" />
      <path d="M8.5 4v5h7V4" />
      <rect x="8.5" y="13" width="7" height="7" rx="1" />
    </svg>
  );
}

/** 警告（确认弹窗） */
export function IconAlert(p: IconProps) {
  return (
    <svg {...svgProps({ size: 18, ...p })}>
      <path d="M12 4.5 21 20H3l9-15.5Z" />
      <path d="M12 10v4" />
      <path d="M12 17h.01" />
    </svg>
  );
}

/** 空态：清单 */
export function IconList(p: IconProps) {
  return (
    <svg {...svgProps(p)}>
      <path d="M8 6h12M8 12h12M8 18h12" />
      <path d="M4 6h.01M4 12h.01M4 18h.01" />
    </svg>
  );
}
