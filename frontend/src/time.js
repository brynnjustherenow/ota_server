import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import "dayjs/locale/zh-cn";

dayjs.extend(relativeTime);
dayjs.locale("zh-cn");

/**
 * 上次活动展示：30 天内 → 相对时间（“5秒前 / 2天前”）；
 * 超过 30 天 → 绝对时间 YYYY-MM-DD HH:mm；无记录 → “从未活动”。
 * @param {number|null} unixSec unix 秒
 */
export function formatLastActive(unixSec) {
  if (!unixSec) return "从未活动";
  const t = dayjs.unix(unixSec);
  if (dayjs().diff(t, "day") > 30) return t.format("YYYY-MM-DD HH:mm");
  return t.fromNow();
}

/**
 * 活动状态灯颜色：1 天内绿；超 1 天黄；超 1 月红；从未活动灰。
 * @param {number|null} unixSec unix 秒
 * @returns {{color:string}} antd 色值
 */
export function activityStatus(unixSec) {
  if (!unixSec) return { color: "#d9d9d9" }; // 灰：从未活动
  const days = dayjs().diff(dayjs.unix(unixSec), "day");
  if (days >= 30) return { color: "#ff4d4f" }; // 红：超一月
  if (days >= 1) return { color: "#faad14" }; // 黄：超一天
  return { color: "#52c41a" }; // 绿：1 天内
}

/**
 * WebSocket 活动事件的中文标签（按 kind 分类）
 * @param {{kind?:string, source:string, event?:string|null}} msg
 */
export function activityLabel(msg) {
  const kind = msg.kind || (msg.source === "video" ? "video" : "activity");
  if (kind === "video") return "视频上传";
  if (kind === "env") return "环境上报";
  if (kind === "record") return "数据上报";
  if (kind === "activity") return "上线";
  // kind === "event"：生命周期事件
  return (
    {
      BOOT: "启动",
      RESET: "复位",
      APP_START: "应用启动",
      NEXT_CYCLE: "轮次",
    }[msg.event] || msg.event || "事件"
  );
}
