import { useEffect, useState, useCallback } from "react";
import {
  Card,
  Form,
  Select,
  Button,
  Space,
  Alert,
  App,
  Typography,
  Tooltip,
  Input,
  Tag,
} from "antd";
import { SettingOutlined, ReloadOutlined, SaveOutlined } from "@ant-design/icons";
import {
  getVersionConfig,
  setVersionConfig,
  listVersions,
} from "../api.js";

const { Paragraph, Text } = Typography;

// 一个常见可下发字段的模板（仅提示用，不会自动填入）
const TEMPLATE_HINT = JSON.stringify(
  {
    app_params: { sleep_interval: 60, patrol_listen_interval_sec: 300 },
    ai_params: { conf_threshold: 0.6, max_record_sec: 15 },
    mqtt: { server: "broker.emqx.io" },
    delivery: { upload: { host: "192.168.1.100", port: 13884 } },
  },
  null,
  2,
);

export default function OtaConfigPage() {
  const { message } = App.useApp();
  const [versions, setVersions] = useState([]);
  const [version, setVersion] = useState(undefined);
  const [text, setText] = useState("{}");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [result, setResult] = useState(null);
  const [hasConfig, setHasConfig] = useState(true);

  const refresh = useCallback(async (autoSelect = true) => {
    try {
      const r = await listVersions();
      if (r.ok) {
        const list = r.data?.data || [];
        setVersions(list.map((v) => ({ label: v, value: v })));
        if (autoSelect && list.length > 0) setVersion(list[0]);
        return list;
      }
    } catch (e) {
      message.error("拉取版本列表失败：" + e.message);
    }
    return [];
  }, [message]);

  useEffect(() => {
    refresh(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 版本变化 → 加载该版本配置
  useEffect(() => {
    if (!version) return;
    (async () => {
      setLoading(true);
      setResult(null);
      try {
        const r = await getVersionConfig(version);
        if (r.ok) {
          const cfg = r.data?.data;
          setHasConfig(true);
          setText(JSON.stringify(cfg ?? {}, null, 2));
        } else if (r.status === 404) {
          setHasConfig(false);
          setText("{}");
        } else {
          message.error(`加载失败：HTTP ${r.status}`);
        }
      } catch (e) {
        message.error("网络错误：" + e.message);
      } finally {
        setLoading(false);
      }
    })();
  }, [version, message]);

  const format = () => {
    try {
      const obj = JSON.parse(text || "{}");
      setText(JSON.stringify(obj, null, 2));
      message.success("已格式化");
    } catch (e) {
      message.error("JSON 解析失败：" + e.message);
    }
  };

  const save = async () => {
    if (!version) return message.warning("请选择版本号");
    let obj;
    try {
      obj = JSON.parse(text || "{}");
    } catch (e) {
      return message.error("JSON 无效：" + e.message);
    }
    if (typeof obj !== "object" || obj === null || Array.isArray(obj)) {
      return message.error("配置必须是 JSON 对象 {}");
    }
    setSaving(true);
    setResult(null);
    try {
      const r = await setVersionConfig(version, obj);
      setResult(r);
      if (r.ok) {
        setHasConfig(true);
        message.success("已保存");
      } else {
        message.error(`保存失败：HTTP ${r.status}`);
      }
    } catch (e) {
      message.error("网络错误：" + e.message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card
      title="OTA 配置下发（随版本 merge）"
      extra={
        <Tooltip title="刷新版本列表">
          <Button
            icon={<ReloadOutlined />}
            onClick={() => refresh(false)}
            size="small"
          />
        </Tooltip>
      }
      bordered={false}
    >
      <Paragraph type="secondary">
        为某版本设置一份 config，服务端每 10 分钟（或手动「广播通知」）会以 retained
        消息 <Text code>{`{cmd:fleet_update, version, config}`}</Text> 下发；设备收到后
        deep_merge 进 <Text code>device_cfg.json</Text>。
      </Paragraph>

      <Form layout="vertical">
        <Form.Item label="版本" required style={{ maxWidth: 320 }}>
          <Select
            showSearch
            placeholder="选择已发布的版本"
            value={version}
            onChange={setVersion}
            options={versions}
            loading={loading}
            notFoundContent={
              versions.length === 0 ? (
                <span style={{ color: "#999" }}>暂无已发布版本</span>
              ) : null
            }
          />
        </Form.Item>

        <Space style={{ marginBottom: 8 }} wrap>
          {version && (
            <Tag color={hasConfig ? "green" : "default"}>
              {hasConfig ? "该版本已有配置" : "该版本暂无配置（保存即创建）"}
            </Tag>
          )}
          <Tag color="red">
            受保护键（即使下发也不覆盖）：device_id / uart_log
          </Tag>
        </Space>

        <Form.Item label="配置（JSON 对象）">
          <Input.TextArea
            value={text}
            onChange={(e) => setText(e.target.value)}
            autoSize={{ minRows: 12, maxRows: 24 }}
            style={{ fontFamily: "monospace" }}
            spellCheck={false}
          />
        </Form.Item>

        <details style={{ marginBottom: 12 }}>
          <summary style={{ cursor: "pointer", color: "#1677ff" }}>
            查看常见字段模板
          </summary>
          <pre
            style={{
              background: "#fafafa",
              padding: 12,
              borderRadius: 6,
              fontSize: 12,
            }}
          >
            {TEMPLATE_HINT}
          </pre>
        </details>

        <Space>
          <Button
            type="primary"
            icon={<SaveOutlined />}
            loading={saving}
            onClick={save}
          >
            保存配置
          </Button>
          <Button icon={<SettingOutlined />} onClick={format}>
            格式化
          </Button>
          <Button
            onClick={() => {
              setText("{}");
              setResult(null);
            }}
          >
            清空
          </Button>
        </Space>

        {result && (
          <Alert
            style={{ marginTop: 16 }}
            type={result.ok ? "success" : "error"}
            showIcon
            message={result.ok ? "保存成功" : `HTTP ${result.status}`}
            description={
              <div className="result-block">
                {JSON.stringify(result.data, null, 2)}
              </div>
            }
          />
        )}
      </Form>
    </Card>
  );
}
