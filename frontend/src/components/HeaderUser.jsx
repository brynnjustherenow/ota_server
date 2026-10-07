import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { App, Avatar, Button, Dropdown, Form, Input, Modal, Space, Tag } from "antd";
import {
  EditOutlined,
  KeyOutlined,
  LogoutOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { changeMyPassword, tokenStore, updateMyProfile } from "../api.js";
import { ROLE_LABELS, useAuth } from "../auth.jsx";

const roleColor = {
  super_admin: "red",
  admin: "orange",
  group_user: "blue",
};

function ProfileModal({ open, onClose }) {
  const { user, setUser } = useAuth();
  const { message } = App.useApp();
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);

  const onFinish = async ({ nickname }) => {
    setSaving(true);
    try {
      const r = await updateMyProfile(nickname);
      if (r.ok && r.data?.code === 200) {
        setUser({ ...user, nickname: r.data.data.nickname });
        message.success("资料已更新");
        onClose();
      } else {
        message.error(r.data?.message || "更新失败");
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title="个人资料"
      open={open}
      onCancel={onClose}
      footer={null}
      destroyOnHidden
    >
      <Form
        form={form}
        onFinish={onFinish}
        initialValues={{ nickname: user?.nickname }}
      >
        <Form.Item label="用户名" name="username" initialValue={user?.username}>
          <Input disabled />
        </Form.Item>
        <Form.Item
          label="昵称"
          name="nickname"
          rules={[{ required: true, message: "请输入昵称" }]}
        >
          <Input placeholder="展示昵称" />
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

function PasswordModal({ open, onClose }) {
  const { user, setUser } = useAuth();
  const { message } = App.useApp();
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);

  const onFinish = async ({ oldPassword, newPassword, confirm }) => {
    if (confirm !== newPassword) {
      message.error("两次输入的新密码不一致");
      return;
    }
    setSaving(true);
    try {
      const r = await changeMyPassword(oldPassword, newPassword);
      if (r.ok && r.data?.code === 200) {
        tokenStore.set(r.data.data.token);
        setUser({ ...user, ...r.data.data.user });
        message.success("密码已修改");
        form.resetFields();
        onClose();
      } else {
        message.error(r.data?.message || "修改失败");
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title="修改密码"
      open={open}
      onCancel={onClose}
      footer={null}
      destroyOnHidden
    >
      <Form form={form} onFinish={onFinish} layout="vertical">
        <Form.Item
          label="原密码"
          name="oldPassword"
          rules={[{ required: true, message: "请输入原密码" }]}
        >
          <Input.Password />
        </Form.Item>
        <Form.Item
          label="新密码"
          name="newPassword"
          rules={[
            { required: true, message: "请输入新密码" },
            { min: 6, message: "至少 6 位" },
          ]}
        >
          <Input.Password />
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
              保存
            </Button>
          </Space>
        </Form.Item>
      </Form>
    </Modal>
  );
}

export default function HeaderUser() {
  const { user, logout } = useAuth();
  const navigate = useNavigate();
  const [profileOpen, setProfileOpen] = useState(false);
  const [pwdOpen, setPwdOpen] = useState(false);

  if (!user) return null;

  const onLogout = () => {
    logout();
    navigate("/login", { replace: true });
  };

  const menuItems = [
    { key: "profile", icon: <EditOutlined />, label: "个人资料" },
    { key: "password", icon: <KeyOutlined />, label: "修改密码" },
    { type: "divider" },
    { key: "logout", icon: <LogoutOutlined />, label: "退出登录" },
  ];

  return (
    <Space size={12}>
      <Tag color={roleColor[user.role]}>{ROLE_LABELS[user.role] || user.role}</Tag>
      <Dropdown
        menu={{
          items: menuItems,
          onClick: ({ key }) => {
            if (key === "profile") setProfileOpen(true);
            else if (key === "password") setPwdOpen(true);
            else if (key === "logout") onLogout();
          },
        }}
      >
        <Space style={{ cursor: "pointer" }}>
          <Avatar size="small" icon={<UserOutlined />} />
          <span>{user.nickname || user.username}</span>
        </Space>
      </Dropdown>
      <ProfileModal open={profileOpen} onClose={() => setProfileOpen(false)} />
      <PasswordModal open={pwdOpen} onClose={() => setPwdOpen(false)} />
    </Space>
  );
}
