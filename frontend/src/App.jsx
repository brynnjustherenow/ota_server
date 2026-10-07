import { useRef } from "react";
import { FloatButton, Layout, Menu, Result, Spin } from "antd";
import {
  CloudOutlined,
  CloudUploadOutlined,
  DesktopOutlined,
  DownloadOutlined,
  FileSearchOutlined,
  HddOutlined,
  NotificationOutlined,
  SettingOutlined,
  TeamOutlined,
  UnorderedListOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import {
  Navigate,
  Outlet,
  Route,
  Routes,
  useLocation,
  useNavigate,
} from "react-router-dom";

import HeaderUser from "./components/HeaderUser.jsx";
import DevicesPage from "./pages/DevicesPage.jsx";
import EnvDataPage from "./pages/EnvDataPage.jsx";
import LoginPage from "./pages/LoginPage.jsx";
import MyDevicesPage from "./pages/MyDevicesPage.jsx";
import OtaConfigPage from "./pages/OtaConfig.jsx";
import OtaDownloadPage from "./pages/OtaDownload.jsx";
import OtaManifestPage from "./pages/OtaManifest.jsx";
import OtaNotifyPage from "./pages/OtaNotify.jsx";
import OtaPublishPage from "./pages/OtaPublish.jsx";
import UsersPage from "./pages/UsersPage.jsx";
import VideoListPage from "./pages/VideoList.jsx";
import VideoUploadPage from "./pages/VideoUpload.jsx";
import { isAdminOrAbove, useAuth } from "./auth.jsx";

const { Sider, Content, Header, Footer } = Layout;

const ADMIN_ROLES = ["super_admin", "admin"];

// roles 省略 = 所有登录用户可见；指定则按角色过滤
const items = [
  { key: "/videos", icon: <UnorderedListOutlined />, label: "视频列表 / 下载" },
  {
    key: "/env",
    icon: <CloudOutlined />,
    label: "环境数据",
  },
  {
    key: "/my-devices",
    icon: <DesktopOutlined />,
    label: "我的设备",
    roles: ["group_user"],
  },
  {
    key: "/video-upload",
    icon: <VideoCameraOutlined />,
    label: "视频上传",
    roles: ADMIN_ROLES,
  },
  { type: "divider", roles: ADMIN_ROLES },
  {
    key: "/devices",
    icon: <HddOutlined />,
    label: "设备管理",
    roles: ADMIN_ROLES,
  },
  {
    key: "/ota/publish",
    icon: <CloudUploadOutlined />,
    label: "OTA 发布",
    roles: ADMIN_ROLES,
  },
  {
    key: "/ota/manifest",
    icon: <FileSearchOutlined />,
    label: "OTA Manifest",
    roles: ADMIN_ROLES,
  },
  {
    key: "/ota/download",
    icon: <DownloadOutlined />,
    label: "OTA 文件下载（ETag 演示）",
    roles: ADMIN_ROLES,
  },
  {
    key: "/ota/config",
    icon: <SettingOutlined />,
    label: "OTA 配置下发",
    roles: ADMIN_ROLES,
  },
  {
    key: "/ota/notify",
    icon: <NotificationOutlined />,
    label: "OTA 广播通知",
    roles: ADMIN_ROLES,
  },
  { type: "divider", roles: ADMIN_ROLES },
  {
    key: "/users",
    icon: <TeamOutlined />,
    label: "用户管理",
    roles: ADMIN_ROLES,
  },
];

/** 未登录 → 跳 /login；token 校验中显示 loading */
function RequireAuth() {
  const { user, loading } = useAuth();
  const location = useLocation();
  if (loading) {
    return (
      <div
        style={{
          height: "100vh",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <Spin size="large" />
      </div>
    );
  }
  if (!user) {
    return <Navigate to="/login" replace state={{ from: location.pathname }} />;
  }
  return <Outlet />;
}

/** 管理员以上才能访问的路由（OTA 全部 / 视频上传 / 用户管理） */
function RequireRole() {
  const { user } = useAuth();
  if (!isAdminOrAbove(user)) {
    return (
      <Result
        status="403"
        title="403"
        subTitle="抱歉，您没有权限访问此页面。"
      />
    );
  }
  return <Outlet />;
}

function ConsoleLayout() {
  const { user } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const contentRef = useRef(null);
  const visibleItems = items.filter(
    (it) => !it.roles || it.roles.includes(user?.role),
  );
  const active =
    visibleItems.find((i) => i.key === location.pathname)?.label ?? "";

  return (
    <Layout style={{ height: "100vh" }}>
      <Sider theme="light" width={260} breakpoint="lg" collapsedWidth={0}>
        <div
          style={{
            height: 56,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            fontWeight: 600,
            fontSize: 16,
            color: "#1677ff",
            borderBottom: "1px solid #f0f0f0",
          }}
        >
          OTA Server Console
        </div>
        <Menu
          mode="inline"
          selectedKeys={[location.pathname]}
          onClick={(e) => navigate(e.key)}
          style={{ borderRight: 0 }}
          items={visibleItems}
        />
      </Sider>
      <Layout>
        <Header
          style={{
            background: "#fff",
            padding: "0 24px",
            fontWeight: 500,
            borderBottom: "1px solid #f0f0f0",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <span>{active}</span>
          <HeaderUser />
        </Header>
        <Content ref={contentRef} style={{ overflow: "auto" }}>
          <div className="page-wrap">
            <Outlet />
          </div>
        </Content>
        <Footer
          style={{
            textAlign: "center",
            background: "#fff",
            padding: "12px 50px",
            borderTop: "1px solid #f0f0f0",
          }}
        >
          <a
            href={import.meta.env.VITE_FOOTER_OFFICIAL_URL}
            target="_blank"
            rel="noopener noreferrer"
          >
            {import.meta.env.VITE_FOOTER_OFFICIAL_LABEL}
          </a>
          {import.meta.env.VITE_FOOTER_ICP && (
            <>
              {"  ·  "}
              <a
                href={import.meta.env.VITE_FOOTER_ICP_URL}
                target="_blank"
                rel="noopener noreferrer"
              >
                {import.meta.env.VITE_FOOTER_ICP}
              </a>
            </>
          )}
        </Footer>
        {/* 回到顶部:页面滚动在 Content 容器上,需指定 target */}
        <FloatButton.BackTop
          target={() => contentRef.current}
          visibilityHeight={200}
          tooltip="回到顶部"
        />
      </Layout>
    </Layout>
  );
}

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<RequireAuth />}>
        <Route path="/" element={<ConsoleLayout />}>
          <Route index element={<Navigate to="/videos" replace />} />
          <Route path="videos" element={<VideoListPage />} />
          <Route path="env" element={<EnvDataPage />} />
          <Route path="my-devices" element={<MyDevicesPage />} />
          <Route element={<RequireRole />}>
            <Route path="video-upload" element={<VideoUploadPage />} />
            <Route path="devices" element={<DevicesPage />} />
            <Route path="ota/publish" element={<OtaPublishPage />} />
            <Route path="ota/manifest" element={<OtaManifestPage />} />
            <Route path="ota/download" element={<OtaDownloadPage />} />
            <Route path="ota/config" element={<OtaConfigPage />} />
            <Route path="ota/notify" element={<OtaNotifyPage />} />
            <Route path="users" element={<UsersPage />} />
          </Route>
          <Route path="*" element={<Navigate to="/videos" replace />} />
        </Route>
      </Route>
    </Routes>
  );
}
