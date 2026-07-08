import { useEffect, useRef, useState } from "react";
import {
  Card,
  Form,
  Input,
  Button,
  Upload,
  Table,
  Progress,
  Alert,
  App,
  Space,
  Typography,
  Tag,
  Select,
} from "antd";
import {
  InboxOutlined,
  CloudUploadOutlined,
  FolderOpenOutlined,
} from "@ant-design/icons";

import { publishOta, listVersions, compareVersions } from "../api.js";

const { Dragger } = Upload;
const { Text } = Typography;

// 默认排除目录名（路径中含任一段即跳过）
const DEFAULT_EXCLUDE_DIRS = [
  ".git",
  ".svn",
  ".hg",
  "node_modules",
  "__pycache__",
  ".idea",
  ".vscode",
  "dist",
  "build",
  "target",
];

export default function OtaPublishPage() {
  const { message, modal } = App.useApp();
  const [version, setVersion] = useState("1.0.1");
  const [items, setItems] = useState([]); // [{uid, file, filename}]
  const [uploading, setUploading] = useState(false);
  const [pct, setPct] = useState(0);
  const [result, setResult] = useState(null);
  const [latestVersion, setLatestVersion] = useState(null);
  const [excludeDirs, setExcludeDirs] = useState(DEFAULT_EXCLUDE_DIRS);

  // 拉取当前最新版本，用于发布前提示
  useEffect(() => {
    (async () => {
      try {
        const r = await listVersions();
        if (r.ok) {
          const list = r.data?.data || [];
          if (list.length > 0) setLatestVersion(list[0]); // 后端已倒序
        }
      } catch {
        // 拉不到也不阻塞用户发布
      }
    })();
  }, []);

  // 文件夹选择 input（需要非标准 webkitdirectory 属性，用 ref + setAttribute）
  const folderInputRef = useRef(null);
  useEffect(() => {
    if (folderInputRef.current) {
      folderInputRef.current.setAttribute("webkitdirectory", "");
      folderInputRef.current.setAttribute("directory", "");
      folderInputRef.current.setAttribute("mozdirectory", "");
    }
  }, []);

  const genUid = () =>
    `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

  // 每个 file 上传时 multipart 的 filename = 用户填的"相对路径"，如 lib/foo.mpy
  const addFiles = (files) => {
    const next = [...items];
    files.forEach((f) => {
      next.push({
        uid: f.uid ?? genUid(),
        file: f.originFileObj ?? f,
        filename: f.name,
      });
    });
    setItems(next);
  };

  // 处理文件夹选择：webkitRelativePath 形如 "ota_pkg/lib/foo.mpy"
  // - 剥掉最顶一层文件夹名，保留 "lib/foo.mpy"
  // - 路径中含任一 excludeDirs 段则跳过（如 ota_pkg/.git/config）
  const onFolderPicked = (e) => {
    const files = Array.from(e.target.files || []);
    if (files.length === 0) return;

    const excludeSet = new Set(excludeDirs.map((s) => s.trim()).filter(Boolean));
    const next = [...items];
    let kept = 0;
    let skipped = 0;

    files.forEach((f) => {
      const rel = f.webkitRelativePath || f.name;
      const segments = rel.split("/");
      // 中间路径段（去掉首段顶层目录、末段文件名）
      const middleSegments = segments.slice(1, -1);
      if (middleSegments.some((s) => excludeSet.has(s))) {
        skipped++;
        return;
      }
      const slashIdx = rel.indexOf("/");
      const stripped =
        slashIdx >= 0 ? rel.slice(slashIdx + 1) || f.name : f.name;
      next.push({ uid: genUid(), file: f, filename: stripped });
      kept++;
    });

    setItems(next);
    e.target.value = "";
    if (skipped > 0) {
      message.success(
        `已添加 ${kept} 个文件；自动跳过 ${skipped} 个被排除目录中的文件`
      );
    } else {
      message.success(`已添加 ${kept} 个文件`);
    }
  };

  const removeItem = (uid) => setItems(items.filter((i) => i.uid !== uid));
  const setFilename = (uid, v) =>
    setItems(items.map((i) => (i.uid === uid ? { ...i, filename: v } : i)));

  const doPublish = async () => {
    setUploading(true);
    setPct(0);
    setResult(null);
    try {
      const r = await publishOta(
        version.trim(),
        items.map((i) => ({ file: i.file, filename: i.filename.trim() })),
        (p) => setPct(Math.round(p * 100))
      );
      setResult(r);
      if (r.ok) {
        message.success("发布成功");
        // 发布成功后刷新 latestVersion
        setLatestVersion(version.trim());
      } else {
        message.error(`发布失败：HTTP ${r.status}`);
      }
    } catch (e) {
      message.error("网络错误：" + e.message);
    } finally {
      setUploading(false);
    }
  };

  const submit = () => {
    if (!version.trim()) return message.warning("请填版本号");
    if (items.length === 0) return message.warning("至少添加一个文件");
    if (items.some((i) => !i.filename.trim()))
      return message.warning("每个文件都要有路径名");

    const v = version.trim();
    // 比对最新版本：v ≤ latest 时弹窗确认
    if (latestVersion && compareVersions(v, latestVersion) <= 0) {
      modal.confirm({
        title: "版本号不大于当前最新版本",
        content: `当前最新版本是 ${latestVersion}，你正要发布 ${v}。同名会覆盖旧版本，确定继续吗？`,
        okText: "仍然发布",
        cancelText: "取消",
        okButtonProps: { danger: true },
        onOk: doPublish,
      });
      return;
    }
    doPublish();
  };

  const columns = [
    {
      title: "本地文件",
      dataIndex: "file",
      key: "file",
      render: (f) => <Text>{f.name}</Text>,
    },
    {
      title: "发布后的相对路径（含子目录）",
      dataIndex: "filename",
      key: "filename",
      render: (v, row) => (
        <Input
          value={v}
          onChange={(e) => setFilename(row.uid, e.target.value)}
          placeholder="如 lib/foo.mpy 或 main.mpy"
          size="small"
        />
      ),
    },
    {
      title: "大小",
      key: "size",
      width: 100,
      render: (_, row) => {
        const n = row.file.size;
        return n >= 1024 ? `${(n / 1024).toFixed(1)} KB` : `${n} B`;
      },
    },
    {
      title: "",
      key: "op",
      width: 80,
      render: (_, row) => (
        <Button type="link" danger size="small" onClick={() => removeItem(row.uid)}>
          移除
        </Button>
      ),
    },
  ];

  return (
    <Card title="OTA 发布（多文件 + 嵌套目录）" bordered={false}>
      <Form layout="vertical">
        <Form.Item
          label={
            <Space>
              <span>版本号</span>
              {latestVersion && (
                <Tag color="blue">当前最新：{latestVersion}</Tag>
              )}
            </Space>
          }
          required
          style={{ maxWidth: 320 }}
        >
          <Input
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            placeholder="如 1.0.1"
          />
        </Form.Item>

        <Form.Item label="选择文件（可多选）">
          <Dragger
            multiple
            showUploadList={false}
            beforeUpload={(file, fileList) => {
              addFiles(fileList);
              return false;
            }}
          >
            <p className="ant-upload-drag-icon">
              <InboxOutlined />
            </p>
            <p className="ant-upload-text">点击或拖拽文件到此处</p>
            <p className="ant-upload-hint">
              可多选；下面表格中可改路径名，包含 `/` 会自动建子目录
            </p>
          </Dragger>
        </Form.Item>

        <Space style={{ marginBottom: 8, alignItems: "center" }} wrap>
          <Button
            icon={<FolderOpenOutlined />}
            onClick={() => folderInputRef.current?.click()}
          >
            选择整个文件夹
          </Button>
          <Tag color="gold">
            按相对路径上传，自动剥掉顶层目录名（如选 ota_pkg/lib/foo.mpy →
            上传为 lib/foo.mpy）
          </Tag>
        </Space>

        <Form.Item
          label="选文件夹时排除的目录名"
          style={{ marginBottom: 16, maxWidth: 640 }}
          tooltip="路径中含任一段即跳过，如 ota_pkg/.git/config 会被排除"
        >
          <Select
            mode="tags"
            value={excludeDirs}
            onChange={setExcludeDirs}
            placeholder="输入目录名后回车，如 .git"
            tokenSeparators={[",", " ", "/"]}
            style={{ width: "100%" }}
          />
        </Form.Item>

        {/* 隐藏的文件夹选择 input */}
        <input
          ref={folderInputRef}
          type="file"
          multiple
          style={{ display: "none" }}
          onChange={onFolderPicked}
        />

        <Table
          rowKey="uid"
          size="small"
          columns={columns}
          dataSource={items}
          pagination={false}
          scroll={{ y: 320 }}
          style={{ marginBottom: 16 }}
        />

        <Space>
          <Button
            type="primary"
            icon={<CloudUploadOutlined />}
            loading={uploading}
            onClick={submit}
          >
            发布
          </Button>
          <Text type="secondary">发布成功后服务端会原子替换 uploads/{version}/</Text>
        </Space>

        {uploading && (
          <div style={{ marginTop: 16 }}>
            <Progress percent={pct} />
          </div>
        )}

        {result && (
          <>
            <Alert
              style={{ marginTop: 16 }}
              type={result.ok ? "success" : "error"}
              message={result.ok ? "发布成功" : `HTTP ${result.status}`}
            />
            <div className="result-block">{JSON.stringify(result.data, null, 2)}</div>
          </>
        )}
      </Form>
    </Card>
  );
}
