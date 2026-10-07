import { useCallback, useEffect, useState } from "react";
import {
  App,
  Button,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Tag,
} from "antd";
import { PlusOutlined, ReloadOutlined } from "@ant-design/icons";
import {
  createUser,
  deleteUser,
  listUsers,
  resetUserPassword,
  updateUser,
} from "../api.js";
import { ROLE_LABELS, useAuth } from "../auth.jsx";

const roleColor = {
  super_admin: "red",
  admin: "orange",
  group_user: "blue",
};

// 角色选项：super_admin 可选全部；admin 只能建组用户
function roleOptions(isSuper) {
  const all = [
    { value: "group_user", label: ROLE_LABELS.group_user },
    { value: "admin", label: ROLE_LABELS.admin },
    { value: "super_admin", label: ROLE_LABELS.super_admin },
  ];
  return isSuper ? all : all.filter((o) => o.value === "group_user");
}

function CreateModal({ open, isSuper, onClose, onDone }) {
  const { message } = App.useApp();
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);

  const onFinish = async (values) => {
    setSaving(true);
    try {
      const r = await createUser({
        username: values.username,
        password: values.password,
        nickname: values.nickname,
        role: values.role,
      });
      if (r.ok && r.data?.code === 200) {
        message.success("用户已创建");
        form.resetFields();
        onDone();
      } else {
        message.error(r.data?.message || "创建失败");
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title="新增用户"
      open={open}
      onCancel={onClose}
      footer={null}
      destroyOnHidden
    >
      <Form
        form={form}
        onFinish={onFinish}
        layout="vertical"
        initialValues={{ role: "group_user", password: "123456" }}
      >
        <Form.Item
          label="用户名"
          name="username"
          rules={[
            { required: true, message: "请输入用户名" },
            { min: 2, max: 32, message: "长度 2-32 个字符" },
            {
              pattern: /^[A-Za-z0-9_-]+$/,
              message: "仅支持字母 / 数字 / _ / -",
            },
          ]}
        >
          <Input placeholder="登录用户名" autoFocus />
        </Form.Item>
        <Form.Item
          label="初始密码"
          name="password"
          extra="已预填默认密码 123456，可直接修改"
          rules={[
            { required: true, message: "请输入初始密码" },
            { min: 6, max: 64, message: "长度 6-64 位" },
          ]}
        >
          <Input.Password placeholder="至少 6 位" />
        </Form.Item>
        <Form.Item
          label="昵称"
          name="nickname"
          extra="未填则与账号同名"
        >
          <Input placeholder="展示昵称（可选）" />
        </Form.Item>
        <Form.Item label="角色" name="role" rules={[{ required: true }]}>
          <Select options={roleOptions(isSuper)} />
        </Form.Item>
        <Form.Item style={{ marginBottom: 0, textAlign: "right" }}>
          <Space>
            <Button onClick={onClose}>取消</Button>
            <Button type="primary" htmlType="submit" loading={saving}>
              创建
            </Button>
          </Space>
        </Form.Item>
      </Form>
    </Modal>
  );
}

function EditModal({ record, isSuper, open, onClose, onDone }) {
  const { message } = App.useApp();
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open && record) {
      form.setFieldsValue({
        nickname: record.nickname,
        role: record.role,
        enabled: record.enabled,
      });
    }
  }, [open, record, form]);

  const onFinish = async (values) => {
    setSaving(true);
    try {
      const r = await updateUser(record.id, values);
      if (r.ok && r.data?.code === 200) {
        message.success("已保存");
        onDone();
      } else {
        message.error(r.data?.message || "保存失败");
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title={`编辑用户：${record?.username ?? ""}`}
      open={open}
      onCancel={onClose}
      footer={null}
      destroyOnHidden
    >
      <Form form={form} onFinish={onFinish} layout="vertical">
        <Form.Item
          label="昵称"
          name="nickname"
          rules={[{ required: true, message: "请输入昵称" }]}
        >
          <Input />
        </Form.Item>
        <Form.Item label="角色" name="role">
          <Select options={roleOptions(isSuper)} disabled={!isSuper} />
        </Form.Item>
        <Form.Item label="启用" name="enabled" valuePropName="checked">
          <Switch checkedChildren="启用" unCheckedChildren="禁用" />
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

function ResetPasswordModal({ record, open, onClose, onDone }) {
  const { message } = App.useApp();
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);

  const onFinish = async ({ newPassword, confirm }) => {
    if (confirm !== newPassword) {
      message.error("两次输入的密码不一致");
      return;
    }
    setSaving(true);
    try {
      const r = await resetUserPassword(record.id, newPassword);
      if (r.ok && r.data?.code === 200) {
        message.success("密码已重置");
        form.resetFields();
        onDone();
      } else {
        message.error(r.data?.message || "重置失败");
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title={`重置密码：${record?.username ?? ""}`}
      open={open}
      onCancel={onClose}
      footer={null}
      destroyOnHidden
    >
      <Form form={form} onFinish={onFinish} layout="vertical">
        <Form.Item
          label="新密码"
          name="newPassword"
          rules={[
            { required: true, message: "请输入新密码" },
            { min: 6, max: 64, message: "长度 6-64 位" },
          ]}
        >
          <Input.Password autoFocus />
        </Form.Item>
        <Form.Item
          label="确认新密码"
          name="confirm"
          rules={[{ required: true, message: "请再次输入新密码" }]}
        >
          <Input.Password />
        </Form.Item>
        <Form.Item style={{ marginBottom: 0, textAlign: "right" }}>
          <Space>
            <Button onClick={onClose}>取消</Button>
            <Button type="primary" htmlType="submit" loading={saving}>
              重置
            </Button>
          </Space>
        </Form.Item>
      </Form>
    </Modal>
  );
}

export default function UsersPage() {
  const { user: me } = useAuth();
  const { message } = App.useApp();
  const [rows, setRows] = useState([]);
  const [loading, setLoading] = useState(false);
  const isSuper = me?.role === "super_admin";

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState(null);
  const [resetting, setResetting] = useState(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const r = await listUsers();
      if (r.ok && r.data?.code === 200) {
        setRows(r.data.data || []);
      } else {
        message.error(r.data?.message || "加载用户列表失败");
      }
    } finally {
      setLoading(false);
    }
  }, [message]);

  useEffect(() => {
    load();
  }, [load]);

  const onDelete = async (record) => {
    const r = await deleteUser(record.id);
    if (r.ok && r.data?.code === 200) {
      message.success(`已删除 ${record.username}`);
      load();
    } else {
      message.error(r.data?.message || "删除失败");
    }
  };

  const columns = [
    { title: "ID", dataIndex: "id", width: 60 },
    { title: "用户名", dataIndex: "username", width: 140 },
    { title: "昵称", dataIndex: "nickname", width: 140 },
    {
      title: "角色",
      dataIndex: "role",
      width: 110,
      render: (v) => <Tag color={roleColor[v]}>{ROLE_LABELS[v] || v}</Tag>,
    },
    {
      title: "状态",
      dataIndex: "enabled",
      width: 90,
      render: (v) =>
        v ? <Tag color="green">启用</Tag> : <Tag color="default">禁用</Tag>,
    },
    { title: "创建时间", dataIndex: "created_at", width: 200, ellipsis: true },
    {
      title: "操作",
      key: "actions",
      render: (_, record) => (
        <Space>
          <Button size="small" onClick={() => setEditing(record)}>
            编辑
          </Button>
          <Button size="small" onClick={() => setResetting(record)}>
            重置密码
          </Button>
          <Popconfirm
            title={`确定删除用户 ${record.username}？`}
            onConfirm={() => onDelete(record)}
            okText="删除"
            okButtonProps={{ danger: true }}
          >
            <Button size="small" danger>
              删除
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          marginBottom: 16,
        }}
      >
        <Space>
          <Button
            type="primary"
            icon={<PlusOutlined />}
            onClick={() => setCreateOpen(true)}
          >
            新增用户
          </Button>
          <Button icon={<ReloadOutlined />} onClick={load}>
            刷新
          </Button>
        </Space>
        <span style={{ color: "#999", alignSelf: "center" }}>
          {isSuper ? "主管理员可管理所有用户" : "管理员仅可管理组用户"}
        </span>
      </div>
      <Table
        rowKey="id"
        columns={columns}
        dataSource={rows}
        loading={loading}
        pagination={{ pageSize: 10, showSizeChanger: false }}
      />
      <CreateModal
        open={createOpen}
        isSuper={isSuper}
        onClose={() => setCreateOpen(false)}
        onDone={() => {
          setCreateOpen(false);
          load();
        }}
      />
      <EditModal
        record={editing}
        isSuper={isSuper}
        open={!!editing}
        onClose={() => setEditing(null)}
        onDone={() => {
          setEditing(null);
          load();
        }}
      />
      <ResetPasswordModal
        record={resetting}
        open={!!resetting}
        onClose={() => setResetting(null)}
        onDone={() => {
          setResetting(null);
          load();
        }}
      />
    </div>
  );
}
