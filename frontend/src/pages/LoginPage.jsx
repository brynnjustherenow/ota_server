import { useEffect } from "react";
import { Navigate, useLocation, useNavigate } from "react-router-dom";
import { Button, Card, Form, Input, App } from "antd";
import { LockOutlined, UserOutlined } from "@ant-design/icons";
import { login, tokenStore } from "../api.js";
import { useAuth } from "../auth.jsx";

export default function LoginPage() {
  const { user, setUser } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const { message } = App.useApp();

  // 已登录直接进控制台
  useEffect(() => {
    if (user) navigate("/", { replace: true });
  }, [user, navigate]);

  if (user) return <Navigate to="/" replace />;

  const onFinish = async ({ username, password }) => {
    const r = await login(username, password);
    if (r.ok && r.data?.code === 200) {
      tokenStore.set(r.data.data.token);
      setUser(r.data.data.user);
      message.success("登录成功");
      const from = location.state?.from;
      navigate(from && from !== "/login" ? from : "/", { replace: true });
    } else {
      message.error(r.data?.message || "登录失败，请检查用户名密码");
    }
  };

  return (
    <div
      style={{
        height: "100vh",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "#f5f5f5",
      }}
    >
      <Card style={{ width: 360 }} variant="outlined">
        <div
          style={{
            textAlign: "center",
            fontWeight: 600,
            fontSize: 20,
            color: "#1677ff",
            marginBottom: 24,
          }}
        >
          OTA Server Console
        </div>
        <Form onFinish={onFinish} size="large" initialValues={{ remember: true }}>
          <Form.Item
            name="username"
            rules={[{ required: true, message: "请输入用户名" }]}
          >
            <Input prefix={<UserOutlined />} placeholder="用户名" autoFocus />
          </Form.Item>
          <Form.Item
            name="password"
            rules={[{ required: true, message: "请输入密码" }]}
          >
            <Input.Password prefix={<LockOutlined />} placeholder="密码" />
          </Form.Item>
          <Form.Item style={{ marginBottom: 0 }}>
            <Button type="primary" htmlType="submit" block>
              登录
            </Button>
          </Form.Item>
        </Form>
      </Card>
    </div>
  );
}
