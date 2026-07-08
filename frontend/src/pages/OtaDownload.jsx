import { useState } from "react";
import {
  Card,
  Form,
  Input,
  Button,
  Space,
  Alert,
  Tag,
  App,
  Typography,
} from "antd";
import { DownloadOutlined } from "@ant-design/icons";
import { downloadOtaFile } from "../api.js";

const { Text, Paragraph } = Typography;

export default function OtaDownloadPage() {
  const { message } = App.useApp();
  const [version, setVersion] = useState("1.0.1");
  const [relpath, setRelpath] = useState("main.mpy");
  const [etag, setEtag] = useState("");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState(null);

  const run = async () => {
    if (!version.trim() || !relpath.trim())
      return message.warning("version 和 relpath 都要填");
    setLoading(true);
    setResult(null);
    try {
      // 用户填的 etag 优先，没填就传 null（不带 If-None-Match）
      const r = await downloadOtaFile(
        version.trim(),
        relpath.trim(),
        etag.trim() || null
      );
      setResult(r);

      if (r.notModified) {
        message.info("服务器返回 304 Not Modified（缓存命中）");
        // 自动更新 etag 输入框，方便二次演示
        if (r.etag) setEtag(r.etag);
      } else if (r.ok) {
        message.success(`下载成功 ${r.size} bytes`);
        // 第一次拿到的 etag 自动回填，方便下一次点击演示 304
        if (r.etag) setEtag(r.etag);
        // 触发浏览器保存
        if (r.blobUrl) {
          const a = document.createElement("a");
          a.href = r.blobUrl;
          a.download = relpath.split("/").pop();
          a.click();
        }
      } else {
        message.error(`下载失败：HTTP ${r.status}`);
      }
    } catch (e) {
      message.error("网络错误：" + e.message);
    } finally {
      setLoading(false);
    }
  };

  return (
    <Card title="OTA 文件下载（ETag / If-None-Match 演示）" bordered={false}>
      <Form layout="vertical">
        <Space style={{ display: "flex", marginBottom: 12 }} wrap>
          <Input
            addonBefore="version"
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            style={{ width: 220 }}
          />
          <Input
            addonBefore="relpath"
            value={relpath}
            onChange={(e) => setRelpath(e.target.value)}
            placeholder="如 main.mpy 或 lib/foo.mpy"
            style={{ width: 280 }}
          />
        </Space>

        <Form.Item
          label={
            <span>
              If-None-Match（ETag）{" "}
              <Tag color="gold">第一次留空，第二次自动填入上次返回的 etag</Tag>
            </span>
          }
        >
          <Input
            value={etag}
            onChange={(e) => setEtag(e.target.value)}
            placeholder='留空则不带 If-None-Match；形如 "d41d8cd9..."'
            style={{ maxWidth: 480 }}
          />
        </Form.Item>

        <Button
          type="primary"
          icon={<DownloadOutlined />}
          loading={loading}
          onClick={run}
        >
          请求
        </Button>

        {result && (
          <div style={{ marginTop: 16 }}>
            {result.notModified ? (
              <Alert
                type="info"
                showIcon
                message="304 Not Modified"
                description={
                  <Space direction="vertical" size={4}>
                    <span>
                      服务端 ETag: <Text code>{result.etag}</Text>
                    </span>
                    <span>字节未传，节省带宽。</span>
                  </Space>
                }
              />
            ) : result.ok ? (
              <Alert
                type="success"
                showIcon
                message={`200 OK · ${result.size} bytes`}
                description={
                  <span>
                    本次返回的 ETag: <Text code>{result.etag || "(无)"}</Text>
                  </span>
                }
              />
            ) : (
              <Alert
                type="error"
                showIcon
                message={`HTTP ${result.status}`}
                description={JSON.stringify(result.data)}
              />
            )}
          </div>
        )}
      </Form>

      <Paragraph type="secondary" style={{ marginTop: 16 }}>
        ETag 取自 manifest 中记录的 MD5。先点一次拿到 ETag，再点一次（带相同的
        If-None-Match）应当返回 304。
      </Paragraph>
    </Card>
  );
}
