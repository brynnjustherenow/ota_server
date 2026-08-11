// 设备 config 字段定义：路径 / 标签 / 类型 / 说明 / 受保护标记。
// 与 device_cfg.json 结构一一对应，用于 OtaConfig 页面渲染「带说明」的结构化表单。
//
// type 取值：
//   - "string"  字符串
//   - "number"  数字
//   - "integer" 整数
//   - "boolean" 开关
//   - "select"  枚举（带 options）
//   - "array"   数组（标签式输入，元素按 itemKind 解析）
//   - "object"  嵌套对象（子字段由 children 渲染）
//   - "json"    任意 JSON（多行文本，原样解析）
//
// protected: true 表示该字段即使下发也不应被设备覆盖（仅展示，禁用编辑或提示）。

function s(path, label, desc, type = "string", extra = {}) {
  return { path, label, desc, type, ...extra };
}

export const PROTECTED_KEYS = new Set([
  "version",
  "device_id",
  "uart_log",
]);

// 按顶层分组的完整字段树。
export const CONFIG_SECTIONS = [
  {
    key: "_root",
    title: "基础信息",
    fields: [
      s("version", "配置版本号", "必填。本份 config 的版本标识，便于追溯（如 2.1.0）。保存/发布时服务端会校验非空。", "string", { required: true }),
      s("device_id", "设备 ID", "设备唯一标识。建议 k230-<序列号>。⚠️ 受保护：下发不会覆盖设备自身 ID。", "string", { protected: true }),
      s("boot_delay_sec", "开机延迟（秒）", "上电后延迟多少秒再启动主程序，便于调试时切入。", "integer"),
      s("run_mode", "运行入口", "启动时执行哪个入口脚本，如 app_main.py。", "string"),
      s("work_mode", "工作模式", "业务模式标识，如 sample / patrol。", "string"),
      s("ota_host", "OTA 主机", "OTA 服务端域名/IP。", "string"),
      s("ota_port", "OTA 端口", "OTA 服务端端口，默认 13884。", "integer"),
      s("ota_path", "OTA 路径", "设备拉取配置清单的路径。", "string"),
    ],
  },
  {
    key: "ota_policy",
    title: "OTA 策略",
    fields: [
      s("ota_policy.check_interval_sec", "检查间隔（秒）", "两次 OTA 检查之间的间隔。86400 = 一天。", "integer"),
    ],
  },
  {
    key: "uart_log",
    title: "串口日志（受保护）",
    fields: [
      s("uart_log.enabled", "启用串口日志", "是否输出串口日志。⚠️ 受保护，下发不覆盖。", "boolean", { protected: true }),
      s("uart_log.board", "串口板型号", "如 01studio_uart3。⚠️ 受保护。", "string", { protected: true }),
      s("uart_log.uart_id", "串口 ID", "使用哪路硬件 UART。⚠️ 受保护。", "integer", { protected: true }),
      s("uart_log.tx_pin", "TX 引脚", "发送引脚号。⚠️ 受保护。", "integer", { protected: true }),
      s("uart_log.rx_pin", "RX 引脚", "接收引脚号。⚠️ 受保护。", "integer", { protected: true }),
      s("uart_log.baudrate", "波特率", "默认 115200。⚠️ 受保护。", "integer", { protected: true }),
      s("uart_log.mirror_usb", "镜像到 USB", "日志同时通过 USB 输出便于调试。⚠️ 受保护。", "boolean", { protected: true }),
    ],
  },
  {
    key: "network",
    title: "网络",
    fields: [
      s("network.lazy_connect", "懒连接", "需要时才建立网络连接，省电。", "boolean"),
      s("network.fast_probe_sec", "快速探测（秒）", "网络快速探测超时。", "integer"),
      s("network.ntp_interval_sec", "NTP 同步间隔（秒）", "多久同步一次时间。", "integer"),
      s("network.ntp_require_bringup", "NTP 必须就绪", "NTP 未成功是否阻塞启动。", "boolean"),
    ],
  },
  {
    key: "storage_policy",
    title: "存储策略",
    fields: [
      s("storage_policy.min_space_mb", "最小预留空间（MB）", "低于此值触发清理。", "integer"),
      s("storage_policy.estimated_clip_mb", "单片段预估（MB）", "单个录像片段估算大小。", "integer"),
      s("storage_policy.max_video_count", "最大视频数", "本地最多保留多少个视频。", "integer"),
      s("storage_policy.evict_fifo_enabled", "启用 FIFO 淘汰", "先进先出清理旧视频。", "boolean"),
    ],
  },
  {
    key: "mqtt",
    title: "MQTT",
    fields: [
      s("mqtt.enabled", "启用 MQTT", "是否启用 MQTT 通信。", "boolean"),
      s("mqtt.server", "Broker 地址", "MQTT 服务器域名/IP。", "string"),
      s("mqtt.port", "Broker 端口", "默认 1883（明文）。", "integer"),
      s("mqtt.client_id", "Client ID", "MQTT 客户端 ID。", "string"),
      s("mqtt.pub_topic", "发布 Topic", "设备上报状态的 Topic。", "string"),
      s("mqtt.sub_topic", "订阅 Topic", "设备接收指令的 Topic。", "string"),
      s("mqtt.always_on", "常连接", "是否保持 MQTT 长连接（费电）。", "boolean"),
      s("mqtt.listen_window_sec", "监听窗口（秒）", "非长连时每次唤醒监听多久。", "integer"),
      s("mqtt.telemetry_interval_sec", "遥测间隔（秒）", "多久上报一次遥测。", "integer"),
    ],
  },
  {
    key: "app_params",
    title: "应用参数",
    fields: [
      s("app_params.record_width", "录像宽", "录像分辨率宽。", "integer"),
      s("app_params.record_height", "录像高", "录像分辨率高。", "integer"),
      s("app_params.record_duration", "录像时长（秒）", "每次触发录像的时长。", "integer"),
      s("app_params.sleep_interval", "休眠间隔（秒）", "两次工作循环之间的休眠时长。", "integer"),
      s("app_params.patrol_listen_interval_sec", "巡逻监听间隔（秒）", "巡逻模式下多久监听一次。", "integer"),
      s("app_params.upload_interval_sec", "上传间隔（秒）", "多久尝试一次视频上传。", "integer"),
      s("app_params.sensor_csi_id", "CSI 传感器 ID", "摄像头 CSI 接口 ID。", "integer"),
      s("app_params.sensor_fps", "传感器帧率", "摄像头输出帧率。", "integer"),
      s("app_params.night_adc_channel", "夜视 ADC 通道", "光敏检测 ADC 通道。", "integer"),
      s("app_params.night_uv_threshold", "夜视 UV 阈值", "判定为夜间的紫外阈值。", "integer"),
      s("app_params.adc0_channel", "ADC0 通道", "通用 ADC0 通道号。", "integer"),
      s("app_params.debug_night_toggle", "调试夜视切换", "调试用：强制切换日夜模式。", "boolean"),
      s("app_params.min_space_mb", "最小预留（MB）", "录像前最小预留空间。", "integer"),
      s("app_params.estimated_clip_mb", "片段预估（MB）", "录像片段预估大小。", "integer"),
      s("app_params.hw_watchdog_timeout_sec", "硬件看门狗（秒）", "硬件看门狗超时，需及时喂狗。", "integer"),
      s("app_params.hang_detect_ms", "挂起检测（毫秒）", "超过此时长未活动判定挂起。", "integer"),
      s("app_params.post_action", "录像后动作", "录像完成后动作，如 reboot / sleep。", "string"),
      s("app_params.gpio_irq_mode_switch", "模式切换 GPIO", "模式切换中断引脚，-1 表示禁用。", "integer"),
      s("app_params.gpio_irq_aux", "辅助中断 GPIO", "辅助中断引脚，-1 表示禁用。", "integer"),
      s("app_params.gpio_irq_edge", "中断触发沿", "中断触发方式：rising / falling / both。", "select", {
        options: [
          { label: "上升沿 rising", value: "rising" },
          { label: "下降沿 falling", value: "falling" },
          { label: "双边沿 both", value: "both" },
        ],
      }),
      s("app_params.gpio_irq_debounce_ms", "中断消抖（毫秒）", "GPIO 中断消抖时长。", "integer"),
      s("app_params.gpio_led1", "LED1 GPIO", "指示灯 1 引脚，-1 表示禁用。", "integer"),
      s("app_params.gpio_led2", "LED2 GPIO", "指示灯 2 引脚，-1 表示禁用。", "integer"),
    ],
  },
  {
    key: "ai_params",
    title: "AI 参数",
    fields: [
      s("ai_params.engine", "推理引擎", "AI 推理引擎，如 yolo11。", "string"),
      s("ai_params.kmodel_path", "模型路径", "kmodel 文件在设备上的绝对路径。", "string"),
      s("ai_params.model_input_size", "模型输入尺寸", "模型输入分辨率 [W, H]。", "array", { itemKind: "integer", length: 2 }),
      s("ai_params.ai_frame_size", "AI 推理帧尺寸", "送入 AI 的帧分辨率 [W, H]。", "array", { itemKind: "integer", length: 2 }),
      s("ai_params.labels", "标签列表", "模型类别标签。", "array", { itemKind: "string" }),
      s("ai_params.target_classes", "目标类别索引", "关注的目标类别索引（从 0 开始）。", "array", { itemKind: "integer" }),
      s("ai_params.conf_threshold", "置信度阈值", "0~1，低于此值的目标被丢弃。", "number"),
      s("ai_params.nms_threshold", "NMS 阈值", "非极大值抑制 IOU 阈值。", "number"),
      s("ai_params.ai_debug", "AI 调试", "开启 AI 调试输出。", "boolean"),
      s("ai_params.always_wait_timeout", "总是等待超时", "检测流程总是等待超时（用于对齐节奏）。", "boolean"),
      s("ai_params.ai_grayscale_input", "灰度输入", "AI 输入使用灰度图。", "boolean"),
      s("ai_params.confirm_frames", "确认帧数", "连续多少帧命中才确认目标。", "integer"),
      s("ai_params.miss_frames", "丢失帧数", "连续多少帧未命中才判定目标离开。", "integer"),
      s("ai_params.ai_check_ms", "AI 检查间隔（毫秒）", "AI 检查循环周期。", "integer"),
      s("ai_params.min_record_sec", "最短录像（秒）", "触发后最短录像时长。", "integer"),
      s("ai_params.max_record_sec", "最长录像（秒）", "单次最长录像时长。", "integer"),
      s("ai_params.record_width", "AI 录像宽", "AI 触发录像分辨率宽。", "integer"),
      s("ai_params.record_height", "AI 录像高", "AI 触发录像分辨率高。", "integer"),
      s("ai_params.sensor_fps", "AI 传感器帧率", "AI 模式下摄像头帧率。", "integer"),
      s("ai_params.ai_fps", "AI 推理帧率", "每秒推理多少帧。", "integer"),
      s("ai_params.max_watch_hours", "最大看守时长（小时）", "0 表示不限制。", "integer"),
      s("ai_params.mem_guard_mb", "内存保护（MB）", "剩余内存低于此值暂停 AI。", "integer"),
    ],
  },
  {
    key: "delivery",
    title: "视频上传（投递）",
    fields: [
      s("delivery.mode", "投递模式", "投递方式，如 upload。", "string"),
      s("delivery.upload.enabled", "启用上传", "是否启用视频上传。", "boolean"),
      s("delivery.upload.upload_async", "异步上传", "异步上传不阻塞录像。", "boolean"),
      s("delivery.upload.host", "上传主机", "上传服务端地址。", "string"),
      s("delivery.upload.tls", "启用 TLS", "是否使用 HTTPS。", "boolean"),
      s("delivery.upload.base_path", "基础路径", "API 基础前缀，如 /Mtpi。", "string"),
      s("delivery.upload.biz", "业务路径", "业务接口路径，如 /mouseVideoUpload。", "string"),
      s("delivery.upload.chunk_size", "分片大小（字节）", "分片上传每片大小，默认 524288。", "integer"),
      s("delivery.upload.delete_after_upload", "上传后删除", "上传成功后删除本地文件。", "boolean"),
      s("delivery.upload.clean_on_fail", "失败时清理", "上传失败时清理分片。", "boolean"),
    ],
  },
  {
    key: "gps",
    title: "GPS",
    fields: [
      s("gps.enabled", "启用 GPS", "是否启用 GPS 定位。", "boolean"),
      s("gps.uart_id", "GPS 串口 ID", "GPS 模块使用的 UART。", "integer"),
      s("gps.tx_pin", "GPS TX 引脚", "GPS 发送引脚。", "integer"),
      s("gps.rx_pin", "GPS RX 引脚", "GPS 接收引脚。", "integer"),
      s("gps.baudrate", "GPS 波特率", "默认 38400。", "integer"),
      s("gps.read_interval_ms", "读取间隔（毫秒）", "GPS 数据读取周期。", "integer"),
    ],
  },
  {
    key: "sensors",
    title: "传感器",
    fields: [
      s("sensors.enabled", "启用传感器", "是否启用温湿度等传感器。", "boolean"),
      s("sensors.poll_interval_sec", "轮询间隔（秒）", "传感器采样周期。", "integer"),
      s("sensors.aht20.enabled", "启用 AHT20", "温湿度传感器。", "boolean"),
      s("sensors.aht20.i2c_id", "AHT20 I2C ID", "I2C 总线号。", "integer"),
      s("sensors.aht20.scl_pin", "AHT20 SCL", "I2C 时钟引脚。", "integer"),
      s("sensors.aht20.sda_pin", "AHT20 SDA", "I2C 数据引脚。", "integer"),
      s("sensors.aht20.freq", "AHT20 频率", "I2C 频率。", "integer"),
      s("sensors.aht20.addr", "AHT20 地址", "I2C 设备地址。", "integer"),
      s("sensors.core_temp.enabled", "启用核心温度", "采集 SoC 核心温度。", "boolean"),
    ],
  },
  {
    key: "env_report",
    title: "环境上报",
    fields: [
      s("env_report.enabled", "启用环境上报", "是否周期上报环境数据。", "boolean"),
      s("env_report.interval_sec", "上报间隔（秒）", "环境数据上报周期。", "integer"),
    ],
  },
];

// ────── 工具：按点分路径读写嵌套对象 ──────

export function getPath(obj, path) {
  if (!path) return obj;
  const parts = path.split(".");
  let cur = obj;
  for (const p of parts) {
    if (cur == null || typeof cur !== "object") return undefined;
    cur = cur[p];
  }
  return cur;
}

export function setPath(obj, path, value) {
  if (!path) return;
  const parts = path.split(".");
  let cur = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    const p = parts[i];
    if (cur[p] == null || typeof cur[p] !== "object") {
      cur[p] = {};
    }
    cur = cur[p];
  }
  cur[parts[parts.length - 1]] = value;
}

export function unsetPath(obj, path) {
  if (!path) return;
  const parts = path.split(".");
  let cur = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    const p = parts[i];
    if (cur[p] == null || typeof cur[p] !== "object") return;
    cur = cur[p];
  }
  delete cur[parts[parts.length - 1]];
}

// 把数组值格式化为字符串（标签输入用）
export function arrayToText(val, itemKind) {
  if (!Array.isArray(val)) return "";
  return val
    .map((v) => (itemKind === "integer" ? Number(v) : String(v)))
    .join(", ");
}

// 把逗号分隔文本解析回数组
export function textToArray(text, itemKind) {
  if (!text) return [];
  return text
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
    .map((s) => (itemKind === "integer" ? Number(s) : s));
}
