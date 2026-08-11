import { useEffect, useState } from "react";
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
  Tag,
  Modal,
} from "antd";
import { NotificationOutlined, ReloadOutlined } from "@ant-design/icons";
import {
  notifyOta,
  listVersions,
  listDeviceConfigs,
  getDeviceConfig,
} from "../api.js";

const { Paragraph, Text } = Typography;

export default function OtaNotifyPage() {
  const { message } = App.useApp();
  const [versions, setVersions] = useState([]);
  const [version, setVersion] = useState(undefined);
  const [devices, setDevices] = useState([]);
  const [deviceId, setDeviceId] = useState(undefined);
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState(null);
  // 「是否携带设备专属配置」选择弹窗
  const [carryOpen, setCarryOpen] = useState(false);

  const refresh = async (autoSelect = true) => {
    try {
      const r = await listVersions();
      if (r.ok) {
        const list = r.data?.data || [];
        setVersions(list.map((v) => ({ label: v, value: v })));
        if (autoSelect && list.length > 0) {
          setVersion(list[0]); // 已是倒序，第一个即最新
        }
        return list;
      }
    } catch (e) {
      message.error("拉取版本列表失败：" + e.message);
    }
    return [];
  };

  // 版本变化 → 拉取该版本下已配置过的 device_id，供「选择设备」下拉
  useEffect(() => {
    if (!version) {
      setDevices([]);
      return;
    }
    (async () => {
      try {
        const r = await listDeviceConfigs(version);
        if (r.ok) setDevices(r.data?.data || []);
      } catch {
        // 忽略
      }
    })();
  }, [version]);

  useEffect(() => {
    refresh(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const run = async () => {
    if (!version) return message.warning("请选择版本号");
    const did = deviceId?.trim() || undefined;

    // 选了 device_id → 先查该设备是否有保存的配置
    if (did) {
      let hasDevCfg = false;
      try {
        const r = await getDeviceConfig(version, did);
        hasDevCfg = r.ok && r.data?.data != null;
      } catch {
        hasDevCfg = false;
      }
      if (hasDevCfg) {
        // 设备有配置 → 弹窗让用户选择是否携带
        setCarryOpen(true);
        return;
      }
      // 设备无配置 → 直接广播（后端自动退回全局配置）
      doBroadcast(did, true);
      return;
    }

    // 未选 device_id → 全 fleet 广播
    doBroadcast(undefined, true);
  };

  // 实际执行广播。carry 仅在 did 非空时有意义。
  const doBroadcast = async (did, carry) => {
    setLoading(true);
    setResult(null);
    try {
      const r = await notifyOta(version, did, did ? carry : undefined);
      setResult(r);
      if (r.ok) {
        message.success(
          did ? `已定向广播到设备 ${did}` : "已广播到全 fleet",
        );
      } else {
        message.error(`广播失败：HTTP ${r.status}`);
      }
    } catch (e) {
      message.error("网络错误：" + e.message);
    } finally {
      setLoading(false);
    }
  };

  // 弹窗选择：carry=true 携带设备配置；carry=false 改用全局
  const handleCarryChoice = (carry) => {
    setCarryOpen(false);
    doBroadcast(deviceId?.trim() || undefined, carry);
  };

  // 设备下拉：已有设备 + 可手动输入新 ID
  const deviceOptions = devices.map((d) => ({ label: d, value: d }));

  return (
    <Card
      title="OTA 广播通知"
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
      <Form layout="vertical">
        <Form.Item label="要广播的版本" required style={{ maxWidth: 320 }}>
          <Select
            showSearch
            placeholder="选择已发布的版本"
            value={version}
            onChange={setVersion}
            options={versions}
            notFoundContent={
              versions.length === 0 ? (
                <span style={{ color: "#999" }}>暂无已发布版本</span>
              ) : null
            }
          />
        </Form.Item>

        <Form.Item
          label={
            <Space size={4}>
              <span>目标设备 ID（可选）</span>
              <Tooltip title="留空 = 广播到所有设备；填入/选择 device_id 时，下发的 MQTT 消息会多一个 device_id 字段，仅匹配的设备会更新。">
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  (留空广播全部；填入则定向)
                </Typography.Text>
              </Tooltip>
            </Space>
          }
          style={{ maxWidth: 320 }}
        >
          <Select
            showSearch
            allowClear
            mode="tags"
            maxCount={1}
            placeholder="留空广播全部，或输入/选择 device_id"
            value={deviceId ? [deviceId] : []}
            onChange={(vals) => setDeviceId(vals?.[0])}
            options={deviceOptions}
            tokenSeparators={[",", " "]}
          />
          {deviceId && (
            <Tag color="blue" style={{ marginTop: 8 }}>
              定向广播：仅设备 {deviceId} 会应用本次更新
            </Tag>
          )}
        </Form.Item>

        <Space>
          <Button
            type="primary"
            icon={<NotificationOutlined />}
            loading={loading}
            onClick={run}
          >
            {deviceId ? "定向广播 MQTT" : "广播 MQTT"}
          </Button>
          <Paragraph type="secondary" style={{ margin: 0 }}>
            服务端以 <Typography.Text code>retained</Typography.Text> 发送：
            {deviceId ? (
              <>
                定向 → <Typography.Text code>{`k230/cam/cmd/${deviceId}`}</Typography.Text>
                （仅该设备订阅，不覆盖全局 retained）
              </>
            ) : (
              <>
                全局 → <Typography.Text code>cmd_topic</Typography.Text>
              </>
            )}
          </Paragraph>
        </Space>

        {result && (
          <Alert
            style={{ marginTop: 16 }}
            type={result.ok ? "success" : "error"}
            showIcon
            message={result.ok ? "广播成功" : `HTTP ${result.status}`}
            description={
              <div className="result-block">
                {JSON.stringify(result.data, null, 2)}
              </div>
            }
          />
        )}
      </Form>

      <Modal
        title={`是否携带设备 ${deviceId} 的专属配置？`}
        open={carryOpen}
        onCancel={() => setCarryOpen(false)}
        maskClosable={false}
        footer={[
          <Button key="cancel" onClick={() => setCarryOpen(false)}>
            取消
          </Button>,
          <Button key="global" onClick={() => handleCarryChoice(false)}>
            改用全局配置
          </Button>,
          <Button
            key="device"
            type="primary"
            loading={loading}
            onClick={() => handleCarryChoice(true)}
          >
            携带设备配置
          </Button>,
        ]}
      >
        <Paragraph>
          检测到设备 <Text code>{deviceId}</Text> 已有保存的专属配置。请选择本次
          定向广播携带的配置来源：
        </Paragraph>
        <ul style={{ paddingLeft: 20, margin: 0 }}>
          <li>
            <Text strong>携带设备配置</Text>：下发该设备专属配置（仅此设备应用）。
          </li>
          <li>
            <Text strong>改用全局配置</Text>：忽略设备专属配置，退回到该版本的全局
            配置；若全局配置不存在则不下发配置（仅版本号 + device_id）。
          </li>
        </ul>
      </Modal>
    </Card>
  );
}
