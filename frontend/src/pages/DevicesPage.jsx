import { useCallback, useEffect, useRef, useState } from "react";
import {
  App,
  Button,
  Form,
  Input,
  Modal,
  Select,
  Space,
  Table,
  Tag,
} from "antd";
import { EditOutlined, ReloadOutlined } from "@ant-design/icons";
import { getDevices, listUsers, setDeviceName, setDeviceOwner } from "../api.js";
import { useActivityWs } from "../useActivityWs.js";
import ActivityCell from "../components/ActivityCell.jsx";

function RenameModal({ record, open, onClose, onDone }) {
  const { message } = App.useApp();
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);

  const onFinish = async ({ name }) => {
    setSaving(true);
    try {
      const r = await setDeviceName(record.device_id, name);
      if (r.ok && r.data?.code === 200) {
        message.success("设备名称已更新");
        onDone();
      } else {
        message.error(r.data?.message || "更新失败");
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title={`编辑设备名称：${record?.device_id ?? ""}`}
      open={open}
      onCancel={onClose}
      footer={null}
      destroyOnHidden
    >
      <Form
        form={form}
        onFinish={onFinish}
        initialValues={{ name: record?.name }}
      >
        <Form.Item
          name="name"
          rules={[{ required: true, message: "请输入设备名称" }]}
          extra="仅用于前端展示，不会同步到硬件"
        >
          <Input placeholder="设备展示名称" maxLength={64} autoFocus />
        </Form.Item>
        <Form.Item style={{ marginBottom: 0, textAlign: "right" }}>
          <Space>
            <Button onClick={onClose}>取消</Button>
            <Button type="primary" htmlType="submit" loading={saving}>
              保存
            </Button>
          </Space>
        </Form.Item>
      </Form>
    </Modal>
  );
}

export default function DevicesPage() {
  const { message } = App.useApp();
  const [rows, setRows] = useState([]);
  const [groupUsers, setGroupUsers] = useState([]);
  const [loading, setLoading] = useState(false);
  const [renaming, setRenaming] = useState(null);
  // 设备活动动画状态：{device_id: {msg, key}}
  const [flashes, setFlashes] = useState({});
  const flashTimers = useRef({});

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [devR, userR] = await Promise.all([getDevices(), listUsers()]);
      if (devR.ok && devR.data?.code === 200) {
        setRows(devR.data.data || []);
      } else {
        message.error(devR.data?.message || "加载设备列表失败");
      }
      if (userR.ok && userR.data?.code === 200) {
        // 归属只能分配给组用户
        setGroupUsers(
          (userR.data.data || []).filter((u) => u.role === "group_user"),
        );
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

  // WebSocket 实时活动：更新行内活动时间 + cell 内 chip 划入动画；未知设备则整表刷新
  useActivityWs(
    useCallback(
      (msg) => {
        setRows((prev) => {
          if (!prev.some((r) => r.device_id === msg.device_id)) {
            load();
            return prev;
          }
          return prev.map((r) =>
            r.device_id === msg.device_id
              ? { ...r, last_active_at: msg.ts }
              : r,
          );
        });
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
      },
      [load],
    ),
  );

  const onOwnerChange = async (record, userId) => {
    const r = await setDeviceOwner(record.device_id, userId ?? null);
    if (r.ok && r.data?.code === 200) {
      message.success(userId ? "已分配归属" : "已解除归属");
      load();
    } else {
      message.error(r.data?.message || "操作失败");
      load();
    }
  };

  const columns = [
    {
      title: "设备 ID",
      dataIndex: "device_id",
      width: 200,
      ellipsis: true,
    },
    {
      title: "名称",
      dataIndex: "name",
      width: 180,
      render: (v) => v || <Tag>未命名</Tag>,
    },
    {
      title: "归属组用户",
      dataIndex: "owner_user_id",
      width: 220,
      render: (_, record) => (
        <Select
          style={{ width: "100%" }}
          placeholder="未分配"
          allowClear
          value={record.owner_user_id ?? undefined}
          onChange={(v) => onOwnerChange(record, v)}
          options={groupUsers.map((u) => ({
            value: u.id,
            label: u.nickname || u.username,
          }))}
        />
      ),
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
    {
      title: "操作",
      key: "actions",
      width: 120,
      render: (_, record) => (
        <Button
          size="small"
          icon={<EditOutlined />}
          onClick={() => setRenaming(record)}
        >
          改名
        </Button>
      ),
    },
  ];

  return (
    <div>
      <div style={{ marginBottom: 16 }}>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={load}>
            刷新
          </Button>
          <span style={{ color: "#999" }}>
            设备列表来自事件/视频上传/手动登记自动合并；名称仅用于展示
          </span>
        </Space>
      </div>
      <Table
        rowKey="device_id"
        columns={columns}
        dataSource={rows}
        loading={loading}
        pagination={{ pageSize: 10, showSizeChanger: false }}
      />
      <RenameModal
        record={renaming}
        open={!!renaming}
        onClose={() => setRenaming(null)}
        onDone={() => {
          setRenaming(null);
          load();
        }}
      />
    </div>
  );
}
