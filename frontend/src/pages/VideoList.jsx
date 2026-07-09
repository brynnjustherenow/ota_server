import { useEffect, useState } from "react";
import {
  Card,
  Select,
  Button,
  Table,
  Space,
  App,
  Tag,
  Typography,
  Modal,
  Tooltip,
  Input,
} from "antd";
import {
  ReloadOutlined,
  DownloadOutlined,
  PlayCircleOutlined,
} from "@ant-design/icons";
import dayjs from "dayjs";
import { listVideos, downloadVideo, listDevices } from "../api.js";
import { API_BASE_URL } from "../api.js";
const { Text } = Typography;

function fmtSize(n) {
  if (n == null) return "-";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i === 0 ? 0 : 2)} ${units[i]}`;
}

export default function VideoListPage() {
  const { message } = App.useApp();
  const [devices, setDevices] = useState([]);
  const [deviceId, setDeviceId] = useState(undefined);
  const [loading, setLoading] = useState(false);
  const [data, setData] = useState([]);
  const [playing, setPlaying] = useState(null); // {filename, url}

  // 拉设备列表
  const refreshDevices = async (autoSelect = true) => {
    try {
      const r = await listDevices();
      if (r.ok) {
        const list = r.data?.data || [];
        setDevices(list.map((d) => ({ label: d, value: d })));
        if (autoSelect && list.length > 0 && !deviceId) {
          setDeviceId(list[0]);
        }
        return list;
      }
    } catch (e) {
      message.error("拉取设备列表失败：" + e.message);
    }
    return [];
  };

  useEffect(() => {
    refreshDevices(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 设备变化时拉视频列表
  useEffect(() => {
    if (deviceId) fetchList(deviceId);
    else setData([]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deviceId]);

  const fetchList = async (id) => {
    setLoading(true);
    try {
      const r = await listVideos(id);
      if (r.ok) {
        setData(r.data?.data || []);
      } else {
        message.error(`查询失败：HTTP ${r.status}`);
        setData([]);
      }
    } catch (e) {
      message.error("网络错误：" + e.message);
    } finally {
      setLoading(false);
    }
  };

  const dl = async (filename) => {
    const r = await downloadVideo(deviceId, filename);
    if (r.ok) message.success(`已开始下载 ${filename}`);
    else message.error(`下载失败：HTTP ${r.status}`);
  };

  const play = (filename) => {
    // 用代理 URL，浏览器原生支持播放
    const url = `${API_BASE_URL}/video/${encodeURIComponent(deviceId)}/${encodeURIComponent(filename)}`;
    setPlaying({ filename, url });
  };

  const columns = [
    {
      title: "文件名",
      dataIndex: "filename",
      key: "filename",
      render: (v) => <Text code>{v}</Text>,
    },
    {
      title: "大小",
      dataIndex: "size",
      key: "size",
      width: 120,
      render: fmtSize,
    },
    {
      title: "修改时间",
      dataIndex: "modified_at",
      key: "modified_at",
      width: 200,
      render: (v) => (v ? dayjs(v).format("YYYY-MM-DD HH:mm:ss") : "-"),
    },
    {
      title: "操作",
      key: "action",
      width: 200,
      render: (_, row) => (
        <Space>
          <Tooltip title="在线播放">
            <Button
              size="small"
              icon={<PlayCircleOutlined />}
              onClick={() => play(row.filename)}
            >
              播放
            </Button>
          </Tooltip>
          <Tooltip title="下载">
            <Button
              size="small"
              icon={<DownloadOutlined />}
              onClick={() => dl(row.filename)}
            />
          </Tooltip>
        </Space>
      ),
    },
  ];

  return (
    <Card
      title={
        <Space>
          <span>视频列表 / 播放</span>
        </Space>
      }
      extra={
        <Button
          icon={<ReloadOutlined />}
          onClick={() => refreshDevices(false)}
          size="small"
        >
          刷新设备
        </Button>
      }
      bordered={false}
    >
      <Space style={{ marginBottom: 16 }} wrap>
        <span>设备：</span>
        <Select
          showSearch
          style={{ width: 240 }}
          placeholder="选择设备"
          value={deviceId}
          onChange={(v) => setDeviceId(v)}
          options={devices}
          notFoundContent={
            devices.length === 0 ? (
              <span style={{ color: "#999" }}>
                暂无设备，先上传一个视频试试
              </span>
            ) : null
          }
        />
        {/* 允许直接手输 device_id，未注册的设备也能查 */}
        <Input.Search
          size="small"
          placeholder="或手动输入 device_id 后回车"
          style={{ width: 220 }}
          enterButton="查询"
          onSearch={(v) => {
            const t = v.trim();
            if (!t) return;
            setDeviceId(t);
          }}
        />
      </Space>

      <Table
        rowKey="filename"
        size="small"
        columns={columns}
        dataSource={data}
        loading={loading}
        pagination={{ pageSize: 20, showSizeChanger: false }}
        locale={{ emptyText: <Tag color="default">暂无视频</Tag> }}
      />

      <Modal
        title={playing?.filename}
        open={!!playing}
        onCancel={() => setPlaying(null)}
        footer={null}
        width={780}
        destroyOnClose
      >
        {playing && (
          <video
            src={playing.url}
            controls
            autoPlay
            style={{ width: "100%", maxHeight: "70vh", background: "#000" }}
          />
        )}
      </Modal>
    </Card>
  );
}
