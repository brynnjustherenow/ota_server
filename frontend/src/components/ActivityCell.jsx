import { useLayoutEffect, useRef, useState } from "react";
import { Badge } from "antd";
import { activityLabel, activityStatus, formatLastActive } from "../time.js";

/**
 * chip 详情文字：超出可视宽度时来回滚动（ping-pong marquee），
 * 溢出量与时长按实测计算，不溢出则静止展示。
 */
function ChipDetail({ text }) {
  const boxRef = useRef(null);
  const textRef = useRef(null);
  const [marquee, setMarquee] = useState(null); // {shift, dur} | null

  useLayoutEffect(() => {
    const box = boxRef.current;
    const t = textRef.current;
    if (!box || !t) return;
    const overflow = t.scrollWidth - box.clientWidth;
    if (overflow > 2) {
      const shift = -(overflow + 6); // 6px 缓冲
      const dur = Math.max(3, Math.abs(shift) / 10); // 每 10px 约 1s
      setMarquee({ shift, dur });
    } else {
      setMarquee(null);
    }
  }, [text]);

  const innerStyle = marquee
    ? {
        ["--marquee-shift"]: `${marquee.shift}px`,
        ["--marquee-dur"]: `${marquee.dur}s`,
      }
    : undefined;

  return (
    <span className="act-chip-detail" ref={boxRef}>
      <span
        ref={textRef}
        className={marquee ? "marquee-inner" : undefined}
        style={innerStyle}
      >
        {text}
      </span>
    </span>
  );
}

/** 事件类型 → antd Badge status(processing 自带呼吸脉冲) */
function statusFor(msg) {
  const kind = msg.kind || (msg.source === "video" ? "video" : "activity");
  if (kind === "event") {
    return (
      { BOOT: "success", RESET: "error", NEXT_CYCLE: "warning" }[msg.event] ||
      "processing"
    );
  }
  if (kind === "record") return "warning";
  return "processing"; // video / env / activity
}

/**
 * 设备活动单元格：状态灯（1天内绿/超1天黄/超1月红/从未活动灰）+ 相对时间；
 * 事件到达时灯脉冲，且从单元格右缘划入一个 antd 状态点 Badge
 * （<Badge status text/>：状态点 + 类型 + 滚动详情）。
 *
 * flash 传整个 WS 消息（含 kind/event/detail），flashKey 变化时通过 key
 * 重挂载元素重新触发 CSS 动画。dot 与 badge 容器是同级元素，key 必须不同
 * （d-/c- 前缀），否则同级 key 冲突会导致 React 复制出多个节点。
 *
 * @param {{lastActiveAt:number|null, flash?:object|null, flashKey?:number}} props
 */
export default function ActivityCell({ lastActiveAt, flash, flashKey = 0 }) {
  const { color } = activityStatus(lastActiveAt);
  return (
    <span className="act-cell">
      <span
        key={`d-${flashKey}`}
        className={`act-dot${flash ? " flash" : ""}`}
        style={{ background: color }}
      />
      <span className="act-time">{formatLastActive(lastActiveAt)}</span>
      {flash && (
        <span key={`c-${flashKey}`} className="act-event-wrap">
          <Badge
            status={statusFor(flash)}
            text={
              <span className="act-event-text">
                <span className="act-event-label">{activityLabel(flash)}</span>
                {flash.detail && <ChipDetail text={String(flash.detail)} />}
              </span>
            }
          />
        </span>
      )}
    </span>
  );
}
