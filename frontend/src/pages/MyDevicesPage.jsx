import { useCallback, useEffect, useRef, useState } from "react";
import { App, Button, Space, Table, Tag } from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import { getDevices } from "../api.js";
import { useActivityWs } from "../useActivityWs.js";
import ActivityCell from "../components/ActivityCell.jsx";

/** 组用户的只读设备列表：名下设备 + 名称 + 上次活动（WebSocket 实时刷新） */
export default function MyDevicesPage() {
  const { message } = App.useApp();
  const [rows, setRows] = useState([]);
  const [loading, setLoading] = useState(false);
  // 设备活动动画状态：{device_id: {msg, key}}
  const [flashes, setFlashes] = useState({});
  const flashTimers = useRef({});

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const r = await getDevices();
      if (r.ok && r.data?.code === 200) {
        setRows(r.data.data || []);
      } else {
        message.error(r.data?.message || "加载设备列表失败");
      }
    } finally {
      setLoading(false);
    }
  }, [message]);

  useEffect(() => {
    load();
  }, [load]);

  // 卸载时清理动画定时器
  useEffect(
    () => () => {
      Object.values(flashTimers.current).forEach(clearTimeout);
    },
    [],
  );

  // WebSocket 实时活动（后端已按归属过滤）：更新活动时间 + cell 内 chip 划入动画
  useActivityWs(
    useCallback((msg) => {
      setRows((prev) =>
        prev.map((r) =>
          r.device_id === msg.device_id ? { ...r, last_active_at: msg.ts } : r,
        ),
      );
      setFlashes((f) => ({
        ...f,
        [msg.device_id]: { msg, key: Date.now() },
      }));
      clearTimeout(flashTimers.current[msg.device_id]);
      flashTimers.current[msg.device_id] = setTimeout(() => {
        setFlashes((f) => {
          const n = { ...f };
          delete n[msg.device_id];
          return n;
        });
      }, 5000);
    }, []),
  );

  const columns = [
    {
      title: "设备 ID",
      dataIndex: "device_id",
      width: 220,
      ellipsis: true,
    },
    {
      title: "名称",
      dataIndex: "name",
      width: 200,
      render: (v) => v || <Tag>未命名</Tag>,
    },
    {
      title: "上次活动",
      dataIndex: "last_active_at",
      width: 300,
      render: (v, record) => {
        const f = flashes[record.device_id];
        return (
          <ActivityCell lastActiveAt={v} flash={f?.msg} flashKey={f?.key} />
        );
      },
    },
  ];

  return (
    <div>
      <div style={{ marginBottom: 16 }}>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={load}>
            刷新
          </Button>
          <span style={{ color: "#999" }}>仅显示分配给您的设备</span>
        </Space>
      </div>
      <Table
        rowKey="device_id"
        columns={columns}
        dataSource={rows}
        loading={loading}
        pagination={false}
      />
    </div>
  );
}
