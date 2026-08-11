import { useEffect, useState, useMemo, useCallback } from "react";
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
  InputNumber,
  Switch,
  Tag,
  Collapse,
  Segmented,
  Empty,
  Divider,
} from "antd";
import {
  SettingOutlined,
  ReloadOutlined,
  SaveOutlined,
  NotificationOutlined,
  PlusOutlined,
  FileTextOutlined,
  DeleteOutlined,
  CodeOutlined,
  FormOutlined,
} from "@ant-design/icons";
import {
  getVersionConfig,
  setVersionConfig,
  notifyOta,
  listVersions,
  getConfigTemplate,
  listDeviceConfigs,
  getDeviceConfig,
  setDeviceConfig,
  deleteDeviceConfig,
  publishDeviceConfig,
} from "../api.js";
import {
  CONFIG_SECTIONS,
  getPath,
  setPath,
  arrayToText,
  textToArray,
} from "../configSchema.js";

const { Paragraph, Text } = Typography;

// 字段类型 → 渲染输入控件
function FieldInput({ field, value, onChange }) {
  if (field.protected) {
    return (
      <Text type="secondary" style={{ fontFamily: "monospace" }}>
        {value === undefined ? "（未设置，受保护不下发）" : JSON.stringify(value)}
      </Text>
    );
  }
  switch (field.type) {
    case "boolean":
      return <Switch checked={!!value} onChange={onChange} />;
    case "integer":
      return (
        <InputNumber
          value={value}
          onChange={(v) => onChange(v ?? 0)}
          style={{ width: "100%" }}
          step={1}
        />
      );
    case "number":
      return (
        <InputNumber
          value={value}
          onChange={(v) => onChange(v ?? 0)}
          style={{ width: "100%" }}
          step={0.01}
        />
      );
    case "select":
      return (
        <Select
          value={value}
          onChange={onChange}
          options={field.options || []}
          allowClear
          style={{ width: "100%" }}
        />
      );
    case "array":
      return (
        <Input
          value={arrayToText(value, field.itemKind)}
          onChange={(e) => onChange(textToArray(e.target.value, field.itemKind))}
          placeholder={`逗号分隔，如 ${field.itemKind === "integer" ? "0, 1, 2" : "mouse, cat"}`}
        />
      );
    default:
      return <Input value={value ?? ""} onChange={(e) => onChange(e.target.value)} />;
  }
}

// 一个字段行：标签 + 说明 tooltip + 输入
function FieldRow({ field, config, onChange }) {
  const value = getPath(config, field.path);
  return (
    <Form.Item
      label={
        <Space size={4}>
          <span>{field.label}</span>
          {field.required && <span style={{ color: "#ff4d4f" }}>*</span>}
          {field.protected && <Tag color="red">受保护</Tag>}
          {field.required && <Tag color="orange">必填</Tag>}
        </Space>
      }
      required={field.required}
      tooltip={field.desc}
      style={{ marginBottom: 12 }}
    >
      <FieldInput
        field={field}
        value={value}
        onChange={(v) => {
          // 受保护字段编辑无效（设备端不接收）
          if (field.protected) return;
          const next = Array.isArray(config)
            ? [...config]
            : { ...config };
          setPath(next, field.path, v);
          onChange(next);
        }}
      />
      <div style={{ fontSize: 12, color: "#999", marginTop: 2 }}>
        <Text code style={{ fontSize: 11 }}>{field.path}</Text>
        {" — "}{field.desc}
      </div>
    </Form.Item>
  );
}

export default function OtaConfigPage() {
  const { message, modal } = App.useApp();
  const [versions, setVersions] = useState([]);
  const [version, setVersion] = useState(undefined);
  const [config, setConfig] = useState({});
  const [text, setText] = useState("{}"); // JSON 模式文本
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [result, setResult] = useState(null);
  const [hasConfig, setHasConfig] = useState(true);

  // scope: fleet（全版本）/ device（设备专属）
  const [scope, setScope] = useState("fleet");
  const [devices, setDevices] = useState([]); // 已配置过的 device_id 列表
  const [deviceId, setDeviceId] = useState(undefined);
  // 视图：form / json
  const [view, setView] = useState("form");

  const refreshVersions = useCallback(async (autoSelect = true) => {
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

  const refreshDevices = useCallback(
    async (v) => {
      if (!v) {
        setDevices([]);
        return;
      }
      try {
        const r = await listDeviceConfigs(v);
        if (r.ok) {
          setDevices(r.data?.data || []);
        }
      } catch {
        // 忽略
      }
    },
    [],
  );

  useEffect(() => {
    refreshVersions(true);
  }, [refreshVersions]);

  // 版本变化 → 加载配置 + 设备列表
  useEffect(() => {
    if (!version) return;
    refreshDevices(version);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version]);

  // scope / deviceId / version 变化 → 重新加载对应配置
  useEffect(() => {
    if (!version) return;
    (async () => {
      setLoading(true);
      setResult(null);
      try {
        let r;
        if (scope === "device") {
          if (!deviceId) {
            setConfig({});
            setText("{}");
            setHasConfig(false);
            return;
          }
          r = await getDeviceConfig(version, deviceId);
        } else {
          r = await getVersionConfig(version);
        }
        if (r.ok) {
          const cfg = r.data?.data ?? {};
          setConfig(cfg);
          setText(JSON.stringify(cfg, null, 2));
          setHasConfig(true);
        } else if (r.status === 404) {
          setConfig({});
          setText("{}");
          setHasConfig(false);
        } else {
          message.error(`加载失败：HTTP ${r.status}`);
        }
      } catch (e) {
        message.error("网络错误：" + e.message);
      } finally {
        setLoading(false);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version, scope, deviceId]);

  // 切到 JSON 视图时同步 config → text
  useEffect(() => {
    if (view === "json") {
      setText(JSON.stringify(config, null, 2));
    } else {
      // 切回 form：尝试解析 text 合并回 config
      try {
        const obj = JSON.parse(text || "{}");
        setConfig(obj);
      } catch {
        // 解析失败保留旧 config
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view]);

  const onConfigChange = (next) => {
    setConfig(next);
    setText(JSON.stringify(next, null, 2));
  };

  // 生成设备模板：拉取 device_cfg.json，可选填入 device_id
  const generateDeviceTemplate = async (targetDeviceId) => {
    try {
      const r = await getConfigTemplate();
      if (!r.ok) {
        message.error(`拉取模板失败：HTTP ${r.status}`);
        return null;
      }
      const tpl = r.data?.data ? JSON.parse(JSON.stringify(r.data.data)) : {};
      if (targetDeviceId) {
        tpl.device_id = targetDeviceId;
      }
      return tpl;
    } catch (e) {
      message.error("网络错误：" + e.message);
      return null;
    }
  };

  // 为当前已选设备按默认模板填充（设备暂无配置时使用）
  const fillTemplateForCurrentDevice = async () => {
    if (!deviceId) return;
    const tpl = await generateDeviceTemplate(deviceId);
    if (!tpl) return;
    setConfig(tpl);
    setText(JSON.stringify(tpl, null, 2));
    message.success(`已为设备 ${deviceId} 生成模板，可编辑后保存/发布`);
  };

  // 校验当前 config
  const validateObj = () => {
    let obj = config;
    if (view === "json") {
      try {
        obj = JSON.parse(text || "{}");
        setConfig(obj);
      } catch (e) {
        message.error("JSON 无效：" + e.message);
        return null;
      }
    }
    if (typeof obj !== "object" || obj === null || Array.isArray(obj)) {
      message.error("配置必须是 JSON 对象 {}");
      return null;
    }
    // 必填：根级 version 非空字符串（全局/设备专属均要求）
    if (typeof obj.version !== "string" || !obj.version.trim()) {
      message.error("config 必填字段 version 缺失或为空");
      return null;
    }
    return obj;
  };

  const save = async () => {
    if (!version) return message.warning("请选择版本号");
    const obj = validateObj();
    if (!obj) return;
    setSaving(true);
    setResult(null);
    try {
      let r;
      if (scope === "device") {
        if (!deviceId) {
          message.warning("请先选择或生成设备");
          setSaving(false);
          return;
        }
        r = await setDeviceConfig(version, deviceId, obj);
      } else {
        r = await setVersionConfig(version, obj);
      }
      setResult(r);
      if (r.ok) {
        setHasConfig(true);
        message.success("已保存");
        if (scope === "device") refreshDevices(version);
      } else {
        message.error(`保存失败：HTTP ${r.status}`);
      }
    } catch (e) {
      message.error("网络错误：" + e.message);
    } finally {
      setSaving(false);
    }
  };

  // 发布 = 保存 + 广播（device 模式带 device_id）
  const publish = async () => {
    if (!version) return message.warning("请选择版本号");
    const obj = validateObj();
    if (!obj) return;
    setPublishing(true);
    setResult(null);
    try {
      let r;
      if (scope === "device") {
        if (!deviceId) {
          message.warning("请先选择或生成设备");
          setPublishing(false);
          return;
        }
        r = await publishDeviceConfig(version, deviceId, obj);
      } else {
        // fleet：先保存再广播
        const sr = await setVersionConfig(version, obj);
        if (!sr.ok) {
          setResult(sr);
          message.error(`保存失败：HTTP ${sr.status}`);
          return;
        }
        r = await notifyOta(version);
      }
      setResult(r);
      if (r.ok) {
        message.success(
          scope === "device"
            ? `已发布到设备 ${deviceId}（仅匹配设备会应用）`
            : "已广播到全 fleet",
        );
        if (scope === "device") refreshDevices(version);
      } else {
        message.error(`发布失败：HTTP ${r.status}`);
      }
    } catch (e) {
      message.error("网络错误：" + e.message);
    } finally {
      setPublishing(false);
    }
  };

  const removeDevice = () => {
    if (!version || !deviceId) return;
    modal.confirm({
      title: "删除设备专属配置",
      content: `确认删除设备 ${deviceId} 在版本 ${version} 下的配置？`,
      okText: "删除",
      cancelText: "取消",
      okButtonProps: { danger: true },
      onOk: async () => {
        try {
          const r = await deleteDeviceConfig(version, deviceId);
          if (r.ok) {
            message.success("已删除");
            setDeviceId(undefined);
            refreshDevices(version);
          } else {
            message.error(`删除失败：HTTP ${r.status}`);
          }
        } catch (e) {
          message.error("网络错误：" + e.message);
        }
      },
    });
  };

  const formatJson = () => {
    try {
      const obj = JSON.parse(text || "{}");
      setConfig(obj);
      setText(JSON.stringify(obj, null, 2));
      message.success("已格式化");
    } catch (e) {
      message.error("JSON 解析失败：" + e.message);
    }
  };

  // 设备下拉选项 = 已有 + 当前选中
  const deviceOptions = useMemo(() => {
    const set = new Set(devices);
    if (deviceId) set.add(deviceId);
    return Array.from(set).map((d) => ({ label: d, value: d }));
  }, [devices, deviceId]);

  return (
    <Card
      title="OTA 配置下发（结构化编辑 + 设备专属模板）"
      extra={
        <Tooltip title="刷新版本列表">
          <Button
            icon={<ReloadOutlined />}
            onClick={() => refreshVersions(false)}
            size="small"
          />
        </Tooltip>
      }
      bordered={false}
    >
      <Paragraph type="secondary">
        为版本设置 config，随 <Text code>fleet_update</Text>（retained）下发后设备会
        deep_merge 进 <Text code>device_cfg.json</Text>。<b>全局配置</b>发到{" "}
        <Text code>k230/cam/cmd</Text>，对整个 fleet 生效；<b>设备专属配置</b>发到{" "}
        <Text code>k230/cam/cmd/{`{device_id}`}</Text>（独立 retained topic，互不覆盖），
        仅匹配的设备会更新。每个字段下方标注了字段路径与含义；标红的「受保护」字段
        （device_id / uart_log）即使下发设备也不会覆盖。
      </Paragraph>

      <Form layout="vertical">
        <Space wrap style={{ marginBottom: 12 }}>
          <Form.Item label="版本" required style={{ marginBottom: 0, minWidth: 200 }}>
            <Select
              showSearch
              placeholder="选择已发布的版本"
              value={version}
              onChange={setVersion}
              options={versions}
              loading={loading}
              style={{ width: 200 }}
              notFoundContent={
                versions.length === 0 ? (
                  <span style={{ color: "#999" }}>暂无已发布版本</span>
                ) : null
              }
            />
          </Form.Item>
          <Form.Item label="作用范围" style={{ marginBottom: 0 }}>
            <Segmented
              value={scope}
              onChange={setScope}
              options={[
                { label: "全局配置 (fleet)", value: "fleet" },
                { label: "设备专属配置", value: "device" },
              ]}
            />
          </Form.Item>
        </Space>

        {scope === "device" && (
          <Space wrap style={{ marginBottom: 12 }} align="center">
            <Form.Item
              label={
                <Space size={4}>
                  <span>device_id</span>
                  <Tooltip title="设备唯一标识。可从下拉选择已配置过的设备，或直接输入一个新的 device_id 创建。选择后：已有配置则自动载入；无配置则下方表单为空并显示「为设备生成模板」按钮。">
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      (选择已有 / 输入新建设备)
                    </Text>
                  </Tooltip>
                </Space>
              }
              style={{ marginBottom: 0, minWidth: 240 }}
            >
              <Select
                showSearch
                mode="tags"
                maxCount={1}
                tokenSeparators={[",", " "]}
                placeholder="选择已有设备，或输入新 device_id"
                value={deviceId ? [deviceId] : []}
                onChange={(vals) => setDeviceId(vals?.[0])}
                options={deviceOptions}
                style={{ width: 260 }}
                allowClear
                notFoundContent={
                  <Empty
                    image={Empty.PRESENTED_IMAGE_SIMPLE}
                    description="暂无已配置设备，可直接输入新 ID"
                  />
                }
              />
            </Form.Item>
            {deviceId && hasConfig && (
              <Button danger icon={<DeleteOutlined />} onClick={removeDevice}>
                删除该设备配置
              </Button>
            )}
            <Tag color={hasConfig ? "green" : "default"}>
              {deviceId
                ? hasConfig
                  ? "该设备已有配置（已自动载入）"
                  : "该设备暂无配置"
                : "请选择或输入设备"}
            </Tag>
          </Space>
        )}

        {scope === "fleet" && (
          <Space style={{ marginBottom: 12 }} wrap>
            {version && (
              <Tag color={hasConfig ? "green" : "default"}>
                {hasConfig ? "该版本已有配置" : "该版本暂无配置（保存即创建）"}
              </Tag>
            )}
            <Tag color="red">
              受保护键（即使下发也不覆盖）：device_id / uart_log
            </Tag>
          </Space>
        )}

        {/* 设备专属 + 已选设备 + 无配置 + 表单为空 → 显示「为设备生成模板」按钮 */}
        {scope === "device" &&
          deviceId &&
          !hasConfig &&
          Object.keys(config).length === 0 && (
            <Alert
              type="info"
              showIcon
              style={{ marginBottom: 12 }}
              message={`设备 ${deviceId} 暂无配置`}
              description="下方表单为空。点击按钮按默认模板（device_cfg.json）填充，模板中的 device_id 会自动设为当前设备，编辑后可保存 / 发布。"
              action={
                <Button
                  type="primary"
                  icon={<PlusOutlined />}
                  onClick={fillTemplateForCurrentDevice}
                  loading={loading}
                >
                  为设备 {deviceId} 生成模板
                </Button>
              }
            />
          )}

        <Space style={{ marginBottom: 12 }} wrap>
          <Segmented
            value={view}
            onChange={setView}
            options={[
              { label: "结构化表单", value: "form", icon: <FormOutlined /> },
              { label: "原始 JSON", value: "json", icon: <CodeOutlined /> },
            ]}
          />
          {view === "json" && (
            <Button
              icon={<FileTextOutlined />}
              onClick={async () => {
                const tpl = await generateDeviceTemplate(
                  scope === "device" ? deviceId : undefined,
                );
                if (tpl) {
                  setConfig(tpl);
                  setText(JSON.stringify(tpl, null, 2));
                  message.success("已载入模板");
                }
              }}
            >
              载入模板覆盖
            </Button>
          )}
        </Space>

        {view === "form" ? (
          <Collapse
            defaultActiveKey={CONFIG_SECTIONS.slice(0, 3).map((s) => s.key)}
            items={CONFIG_SECTIONS.map((sec) => ({
              key: sec.key,
              label: (
                <Space>
                  <Text strong>{sec.title}</Text>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {sec.fields.length} 字段
                  </Text>
                </Space>
              ),
              children: (
                <div style={{ maxWidth: 720 }}>
                  {sec.fields.map((f) => (
                    <FieldRow
                      key={f.path}
                      field={f}
                      config={config}
                      onChange={onConfigChange}
                    />
                  ))}
                </div>
              ),
            }))}
          />
        ) : (
          <>
            <Input.TextArea
              value={text}
              onChange={(e) => setText(e.target.value)}
              autoSize={{ minRows: 16, maxRows: 32 }}
              style={{ fontFamily: "monospace" }}
              spellCheck={false}
            />
            <Button
              icon={<SettingOutlined />}
              onClick={formatJson}
              style={{ marginTop: 8 }}
            >
              格式化
            </Button>
          </>
        )}

        <Divider style={{ margin: "16px 0" }} />

        <Space wrap>
          <Button
            type="primary"
            icon={<SaveOutlined />}
            loading={saving}
            onClick={save}
          >
            保存配置
          </Button>
          <Button
            type="primary"
            danger
            icon={<NotificationOutlined />}
            loading={publishing}
            onClick={publish}
          >
            {scope === "device"
              ? `保存并发布到设备${deviceId ? `（${deviceId}）` : ""}`
              : "保存并广播到 fleet"}
          </Button>
          <Button
            onClick={() => {
              setConfig({});
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
            message={result.ok ? "操作成功" : `HTTP ${result.status}`}
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
