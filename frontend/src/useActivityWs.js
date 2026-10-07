import { useEffect, useRef } from "react";
import { wsUrl } from "./api.js";

/**
 * 设备活动 WebSocket 订阅。
 * 断线自动重连（2s 退避）；30s 发一次 ping 保活；卸载时关闭。
 * @param {(msg:{device_id:string, source:string, event:string|null, detail:string|null, ts:number})=>void} onActivity
 * @param {boolean} [enabled=true] 无 token 时不开
 */
export function useActivityWs(onActivity, enabled = true) {
  const cbRef = useRef(onActivity);
  cbRef.current = onActivity;

  useEffect(() => {
    if (!enabled) return;
    let ws = null;
    let closed = false;
    let retryTimer = null;
    let pingTimer = null;

    const cleanup = () => {
      closed = true;
      clearTimeout(retryTimer);
      clearInterval(pingTimer);
      if (ws) {
        ws.onclose = null;
        ws.close();
      }
    };

    const connect = () => {
      ws = new WebSocket(wsUrl());
      ws.onopen = () => {
        pingTimer = setInterval(() => {
          if (ws?.readyState === WebSocket.OPEN) ws.send("ping");
        }, 30000);
      };
      ws.onmessage = (e) => {
        try {
          const msg = JSON.parse(e.data);
          if (msg && msg.device_id) cbRef.current(msg);
        } catch {
          /* 忽略非 JSON 消息 */
        }
      };
      ws.onclose = () => {
        clearInterval(pingTimer);
        if (!closed) retryTimer = setTimeout(connect, 2000);
      };
    };

    connect();
    return cleanup;
  }, [enabled]);
}
