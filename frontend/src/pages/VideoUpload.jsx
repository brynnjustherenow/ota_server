import { useState } from "react";
import {
  Card,
  Form,
  Input,
  Button,
  Upload,
  Progress,
  Alert,
  App,
  Row,
  Col,
} from "antd";
import { InboxOutlined, UploadOutlined } from "@ant-design/icons";
import { uploadVideo } from "../api.js";

const { Dragger } = Upload;

export default function VideoUploadPage() {
  const { message } = App.useApp();
  const [deviceId, setDeviceId] = useState("k230-001");
  const [file, setFile] = useState(null);
  const [filename, setFilename] = useState("");
  const [pct, setPct] = useState(0);
  const [uploading, setUploading] = useState(false);
  const [result, setResult] = useState(null);

  const beforeUpload = (f) => {
    setFile(f);
    setFilename(f.name);
    return false; // 阻止 antd 自动上传
  };

  const submit = async () => {
    if (!deviceId.trim()) return message.warning("请填 device_id");
    if (!file) return message.warning("请先选文件");
    setUploading(true);
    setPct(0);
    setResult(null);
    try {
      const r = await uploadVideo(deviceId.trim(), file, filename.trim() || file.name, (p) =>
        setPct(Math.round(p * 100))
      );
      setResult(r);
      if (r.ok) message.success("上传成功");
      else message.error(`上传失败：HTTP ${r.status}`);
    } catch (e) {
      message.error("网络错误：" + e.message);
    } finally {
      setUploading(false);
    }
  };

  return (
    <Card title="上传视频" bordered={false}>
      <Form layout="vertical">
        <Row gutter={16}>
          <Col span={8}>
            <Form.Item label="Device ID" required>
              <Input
                value={deviceId}
                onChange={(e) => setDeviceId(e.target.value)}
                placeholder="如 k230-001"
              />
            </Form.Item>
          </Col>
          <Col span={16}>
            <Form.Item label="自定义文件名（缺省用原文件名）">
              <Input
                value={filename}
                onChange={(e) => setFilename(e.target.value)}
                placeholder="如 20260707_143022.mp4"
              />
            </Form.Item>
          </Col>
        </Row>

        <Form.Item label="视频文件">
          <Dragger
            accept="video/*"
            beforeUpload={beforeUpload}
            maxCount={1}
            fileList={file ? [{ uid: "-1", name: file.name }] : []}
            onRemove={() => {
              setFile(null);
              setFilename("");
            }}
          >
            <p className="ant-upload-drag-icon">
              <InboxOutlined />
            </p>
            <p className="ant-upload-text">点击或拖拽视频到此处</p>
            <p className="ant-upload-hint">通过 raw body 上传，文件名由 Content-Disposition 携带</p>
          </Dragger>
        </Form.Item>

        <Button
          type="primary"
          icon={<UploadOutlined />}
          loading={uploading}
          onClick={submit}
        >
          开始上传
        </Button>

        {uploading && (
          <div style={{ marginTop: 16 }}>
            <Progress percent={pct} />
          </div>
        )}

        {result && (
          <>
            {result.ok ? (
              <Alert
                style={{ marginTop: 16 }}
                type="success"
                message="上传成功"
                description={
                  <div>
                    size: {result.data?.data?.size} bytes &nbsp;|&nbsp; md5:{" "}
                    <code>{result.data?.data?.md5}</code>
                  </div>
                }
              />
            ) : (
              <Alert
                style={{ marginTop: 16 }}
                type="error"
                message={`HTTP ${result.status}`}
                description={JSON.stringify(result.data, null, 2)}
              />
            )}
            <div className="result-block">{JSON.stringify(result.data, null, 2)}</div>
          </>
        )}
      </Form>
    </Card>
  );
}
