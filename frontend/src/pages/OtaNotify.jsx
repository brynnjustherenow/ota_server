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
} from "antd";
import { NotificationOutlined, ReloadOutlined } from "@ant-design/icons";
import { notifyOta, listVersions } from "../api.js";

const { Paragraph } = Typography;

export default function OtaNotifyPage() {
  const { message } = App.useApp();
  const [versions, setVersions] = useState([]);
  const [version, setVersion] = useState(undefined);
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState(null);

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

  useEffect(() => {
    refresh(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const run = async () => {
    if (!version) return message.warning("请选择版本号");
    setLoading(true);
    setResult(null);
    try {
      const r = await notifyOta(version);
      setResult(r);
      if (r.ok) message.success("广播成功");
      else message.error(`广播失败：HTTP ${r.status}`);
    } catch (e) {
      message.error("网络错误：" + e.message);
    } finally {
      setLoading(false);
    }
  };

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

        <Space>
          <Button
            type="primary"
            icon={<NotificationOutlined />}
            loading={loading}
            onClick={run}
          >
            广播 MQTT
          </Button>
          <Paragraph type="secondary" style={{ margin: 0 }}>
            服务端会向{" "}
            <Typography.Text code>cmd_topic</Typography.Text> 推送{" "}
            <Typography.Text code>{`{ts, version}`}</Typography.Text>
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
    </Card>
  );
}
