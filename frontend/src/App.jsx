import { useState } from "react";
import { Layout, Menu } from "antd";
import {
  VideoCameraOutlined,
  UnorderedListOutlined,
  CloudUploadOutlined,
  FileSearchOutlined,
  DownloadOutlined,
  NotificationOutlined,
} from "@ant-design/icons";

import VideoUploadPage from "./pages/VideoUpload.jsx";
import VideoListPage from "./pages/VideoList.jsx";
import OtaPublishPage from "./pages/OtaPublish.jsx";
import OtaManifestPage from "./pages/OtaManifest.jsx";
import OtaDownloadPage from "./pages/OtaDownload.jsx";
import OtaNotifyPage from "./pages/OtaNotify.jsx";

const { Sider, Content, Header } = Layout;

const items = [
  { key: "video-upload", icon: <VideoCameraOutlined />, label: "视频上传" },
  { key: "video-list", icon: <UnorderedListOutlined />, label: "视频列表 / 下载" },
  { type: "divider" },
  { key: "ota-publish", icon: <CloudUploadOutlined />, label: "OTA 发布" },
  { key: "ota-manifest", icon: <FileSearchOutlined />, label: "OTA Manifest" },
  { key: "ota-download", icon: <DownloadOutlined />, label: "OTA 文件下载（ETag 演示）" },
  { key: "ota-notify", icon: <NotificationOutlined />, label: "OTA 广播通知" },
];

export default function App() {
  const [active, setActive] = useState("video-upload");

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
          selectedKeys={[active]}
          onClick={(e) => setActive(e.key)}
          style={{ borderRight: 0 }}
          items={items}
        />
      </Sider>
      <Layout>
        <Header
          style={{
            background: "#fff",
            padding: "0 24px",
            fontWeight: 500,
            borderBottom: "1px solid #f0f0f0",
          }}
        >
          {items.find((i) => i.key === active)?.label}
        </Header>
        <Content style={{ overflow: "auto" }}>
          <div className="page-wrap">
            {active === "video-upload" && <VideoUploadPage />}
            {active === "video-list" && <VideoListPage />}
            {active === "ota-publish" && <OtaPublishPage />}
            {active === "ota-manifest" && <OtaManifestPage />}
            {active === "ota-download" && <OtaDownloadPage />}
            {active === "ota-notify" && <OtaNotifyPage />}
          </div>
        </Content>
      </Layout>
    </Layout>
  );
}
