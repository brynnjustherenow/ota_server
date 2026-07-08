import { useState } from "react";
import {
  Card,
  Form,
  Input,
  Button,
  Table,
  Space,
  Tag,
  Typography,
  App,
  Empty,
  Descriptions,
} from "antd";
import { FileSearchOutlined } from "@ant-design/icons";
import dayjs from "dayjs";
import { getManifest } from "../api.js";

const { Text, Paragraph } = Typography;

function fmtSize(n) {
  if (!n) return "-";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i === 0 ? 0 : 2)} ${units[i]}`;
}

export default function OtaManifestPage() {
  const { message } = App.useApp();
  const [version, setVersion] = useState("1.0.1");
  const [loading, setLoading] = useState(false);
  const [meta, setMeta] = useState(null);

  const fetchIt = async () => {
    if (!version.trim()) return message.warning("请填版本号");
    setLoading(true);
    setMeta(null);
    try {
      const r = await getManifest(version.trim());
      if (r.ok) {
        setMeta(r.data);
        message.success("已加载 manifest");
      } else {
        message.error(`加载失败：HTTP ${r.status} ${r.data?.message || ""}`);
      }
    } catch (e) {
      message.error("网络错误：" + e.message);
    } finally {
      setLoading(false);
    }
  };

  const columns = [
    {
      title: "name",
      dataIndex: "name",
      key: "name",
      render: (v) => <Text code>{v}</Text>,
    },
    {
      title: "path",
      dataIndex: "path",
      key: "path",
      width: 160,
      render: (v) => (v ? <Tag color="blue">{v}</Tag> : <Tag>root</Tag>),
    },
    {
      title: "size",
      dataIndex: "size",
      key: "size",
      width: 120,
      render: fmtSize,
    },
    {
      title: "md5",
      dataIndex: "md5",
      key: "md5",
      render: (v) => (
        <Text code copyable style={{ fontSize: 12 }}>
          {v}
        </Text>
      ),
    },
  ];

  return (
    <Card title="OTA Manifest" bordered={false}>
      <Space style={{ marginBottom: 16 }}>
        <Input
          value={version}
          onChange={(e) => setVersion(e.target.value)}
          placeholder="version"
          style={{ width: 240 }}
          onPressEnter={fetchIt}
        />
        <Button
          type="primary"
          icon={<FileSearchOutlined />}
          loading={loading}
          onClick={fetchIt}
        >
          拉取 manifest
        </Button>
      </Space>

      {meta ? (
        <>
          <Descriptions
            size="small"
            bordered
            column={3}
            style={{ marginBottom: 16 }}
          >
            <Descriptions.Item label="version">
              {meta.version}
            </Descriptions.Item>
            <Descriptions.Item label="file_count">
              {meta.file_count}
            </Descriptions.Item>
            <Descriptions.Item label="create_at">
              {meta.create_at
                ? dayjs(meta.create_at).format("YYYY-MM-DD HH:mm:ss")
                : "-"}
            </Descriptions.Item>
          </Descriptions>

          <Table
            rowKey={(r) => `${r.path}/${r.name}`}
            size="small"
            columns={columns}
            dataSource={meta.files || []}
            pagination={false}
          />
        </>
      ) : (
        <Empty description="尚未加载" />
      )}

      <Paragraph type="secondary" style={{ marginTop: 16 }}>
        Tip: 这里展示的 <Text code>path</Text> 和 <Text code>size</Text> 是
        ota_publish 时记录的，可作为客户端预判存储 / 校验完整性的依据。
      </Paragraph>
    </Card>
  );
}
