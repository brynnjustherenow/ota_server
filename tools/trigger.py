#!/usr/bin/env python3
"""设备事件触发器 - OTA Server 联调用。

交互式:输入数字触发不同类型的设备活动,用于测试 WebSocket 实时推送、
活动状态灯动画与右侧滑入通知。

用法:
    python trigger.py                 # 交互模式,默认设备 cam-001
    python trigger.py -d eagle-001    # 指定默认设备

依赖: pip install paho-mqtt
后端地址/MQTT broker 可改下方常量。
"""
import json
import random
import sys
import time
import urllib.request

BACKEND = "http://127.0.0.1:13884"
BROKER_HOST = "broker.emqx.io"
BROKER_PORT = 1883
TOPIC = "k230/cam/status"

try:
    import paho.mqtt.client as mqtt
except ImportError:
    print("缺少依赖: pip install paho-mqtt")
    sys.exit(1)


class MqttPub:
    """会话级 MQTT 连接,触发即时生效。"""

    def __init__(self):
        self.client = None

    def _ensure(self):
        if self.client is not None and self.client.is_connected():
            return
        c = mqtt.Client(
            mqtt.CallbackAPIVersion.VERSION2,
            client_id=f"trigger-{random.randint(10000, 99999)}",
        )
        c.connect(BROKER_HOST, BROKER_PORT, keepalive=60)
        c.loop_start()
        for _ in range(30):  # 最多等 3s 连接建立
            if c.is_connected():
                self.client = c
                return
            time.sleep(0.1)
        raise RuntimeError(f"MQTT 连接 {BROKER_HOST}:{BROKER_PORT} 超时")

    def publish(self, payload: dict):
        self._ensure()
        info = self.client.publish(TOPIC, json.dumps(payload), qos=1)
        info.wait_for_publish(timeout=5)
        if not info.is_published():
            raise RuntimeError("MQTT publish 未确认(检查 broker 连通性)")

    def close(self):
        if self.client:
            try:
                self.client.disconnect()
                self.client.loop_stop()
            except Exception:
                pass


def http_upload_video(device_id: str, size_kb: int = 64):
    """绕过系统代理上传 localhost(本机代理 7890 会拦 127.0.0.1)。"""
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    data = random.randbytes(size_kb * 1024)
    req = urllib.request.Request(
        f"{BACKEND}/video/{device_id}",
        data=data,
        method="POST",
        headers={
            "Content-Disposition": (
                f'attachment; filename="trigger_{int(time.time())}.mp4"'
            )
        },
    )
    with opener.open(req, timeout=15) as resp:
        return json.loads(resp.read())


MENU = """
--------------------
当前设备: {dev}
 1) 视频上传 (HTTP)
 2) BOOT        启动
 3) RESET       复位
 4) APP_START   应用启动
 5) NEXT_CYCLE  轮次
 6) env         环境上报
 7) record      数据上报
 8) 无ev消息    普通上线
 9) 自定义JSON直发
 d) 切换设备
 0) 退出
--------------------"""


def main():
    dev = "cam-001"
    if "-d" in sys.argv:
        dev = sys.argv[sys.argv.index("-d") + 1]
    pub = MqttPub()
    try:
        while True:
            print(MENU.format(dev=dev))
            try:
                choice = input("请选择: ").strip()
            except (EOFError, KeyboardInterrupt):
                break
            if choice in ("0", "q", "quit", "exit"):
                break
            if choice == "d":
                nd = input(f"设备ID [{dev}]: ").strip()
                if nd:
                    dev = nd
                continue
            try:
                if choice == "1":
                    body = http_upload_video(dev)
                    d = body.get("data") or {}
                    print(f"[OK] 视频上传 {dev}: {d.get('filename')} ({d.get('size')}B)")
                elif choice in ("2", "3", "4", "5"):
                    ev = {
                        "2": "BOOT",
                        "3": "RESET",
                        "4": "APP_START",
                        "5": "NEXT_CYCLE",
                    }[choice]
                    payload = {
                        "device_id": dev,
                        "ev": ev,
                        "ev_detail": "trigger手动触发",
                    }
                    if ev == "NEXT_CYCLE":
                        payload["record_count"] = random.randint(1, 99)
                    pub.publish(payload)
                    print(f"[OK] MQTT {ev} -> {dev}")
                elif choice == "6":
                    pub.publish(
                        {
                            "device_id": dev,
                            "env": {
                                "temp": round(random.uniform(15, 35), 1),
                                "humi": random.randint(30, 90),
                                "bat": random.randint(30, 100),
                            },
                        }
                    )
                    print(f"[OK] MQTT 环境上报 -> {dev}")
                elif choice == "7":
                    pub.publish(
                        {
                            "device_id": dev,
                            "record": {
                                "seq": random.randint(1, 999),
                                "count": random.randint(0, 20),
                            },
                        }
                    )
                    print(f"[OK] MQTT 数据上报 -> {dev}")
                elif choice == "8":
                    pub.publish({"device_id": dev, "ts": int(time.time())})
                    print(f"[OK] MQTT 普通消息 -> {dev}")
                elif choice == "9":
                    raw = input("JSON: ").strip()
                    obj = json.loads(raw)
                    obj.setdefault("device_id", dev)
                    pub.publish(obj)
                    print(f"[OK] MQTT 直发 -> {dev}: {obj}")
                else:
                    print("无效选项")
            except Exception as e:
                print(f"[FAIL] {e}")
    finally:
        pub.close()
    print("bye")


if __name__ == "__main__":
    main()
