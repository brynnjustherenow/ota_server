import { useCallback, useEffect, useMemo, useState } from "react";
import {
  App,
  Button,
  DatePicker,
  Descriptions,
  Select,
  Space,
  Table,
  Tag,
} from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import dayjs from "dayjs";
import { getDevices, listEnv } from "../api.js";

const { RangePicker } = DatePicker;

/** env 字段中文标签(硬件 AGENTS.md 6.5;未列出的字段展示原始键名) */
const FIELD_LABELS = {
  type: "消息类型",
  device_id: "设备 ID",
  ts: "上报时间戳",
  temp_c: "温度",
  hum_pct: "湿度",
  core_temp_c: "核心温度",
  weight_g: "重量",
  adc0_u16: "ADC0 原始值",
  adc0_v: "ADC0 电压",
  adc1_u16: "ADC1 原始值",
  adc1_v: "ADC1 电压",
  imei: "IMEI",
  imsi: "IMSI",
  iccid: "ICCID",
  model: "模组型号",
};

/** 展开行的字段排序:重要的在前,gps_* 与未知字段排后 */
const FIELD_ORDER = [
  "ts",
  "temp_c",
  "hum_pct",
  "core_temp_c",
  "weight_g",
  "adc0_v",
  "adc0_u16",
  "adc1_v",
  "adc1_u16",
  "model",
  "imei",
  "imsi",
  "iccid",
  "device_id",
  "type",
];

function fieldLabel(key) {
  if (FIELD_LABELS[key]) return FIELD_LABELS[key];
  if (key.startsWith("gps_")) return `GPS ${key.slice(4)}`;
  return key;
}

/** 字段单位(值后缀) */
const FIELD_UNITS = {
  temp_c: "°C",
  hum_pct: "%",
  core_temp_c: "°C",
  weight_g: "g",
  adc0_v: "V",
  adc1_v: "V",
};

/** 摘要列字段(存在才展示,详情看展开行) */
const SUMMARY_KEYS = [
  { key: "temp_c", label: "温度", color: "orange" },
  { key: "hum_pct", label: "湿度", color: "blue" },
  { key: "weight_g", label: "重量", color: "purple" },
];

function PayloadSummary({ payload }) {
  if (!payload || typeof payload !== "object") return "-";
  const tags = SUMMARY_KEYS.filter((s) => payload[s.key] != null).map((s) => (
    <Tag key={s.key} color={s.color}>
      {s.label} {payload[s.key]}
      {FIELD_UNITS[s.key] || ""}
    </Tag>
  ));
  return tags.length ? <Space size={4} wrap>{tags}</Space> : "-";
}

/** 展开行:env 全量字段(中文 label,重要字段在前,带单位) */
function ExpandedPayload({ payload }) {
  if (!payload || typeof payload !== "object" || Object.keys(payload).length === 0) {
    return <span style={{ color: "#999" }}>无数据</span>;
  }
  const keys = Object.keys(payload).sort((a, b) => {
    const ia = FIELD_ORDER.indexOf(a);
    const ib = FIELD_ORDER.indexOf(b);
    if (ia !== -1 && ib !== -1) return ia - ib;
    if (ia !== -1) return -1;
    if (ib !== -1) return 1;
    return a.localeCompare(b);
  });
  const items = keys.map((k) => {
    const v = payload[k];
    const unit = FIELD_UNITS[k] || "";
    return {
      key: k,
      label: fieldLabel(k),
      children:
        v !== null && typeof v === "object" ? (
          <code style={{ fontSize: 12 }}>{JSON.stringify(v)}</code>
        ) : (
          `${v}${unit}`
        ),
    };
  });
  return (
    <Descriptions
      size="small"
      bordered
      column={3}
      items={items}
      style={{ maxWidth: 1000 }}
    />
  );
}

export default function EnvDataPage() {
  const { message } = App.useApp();
  const [devices, setDevices] = useState([]);
  const [rows, setRows] = useState([]);
  const [loading, setLoading] = useState(false);
  const [deviceId, setDeviceId] = useState(undefined);
  const [range, setRange] = useState(null); // [Dayjs, Dayjs] | null

  useEffect(() => {
    getDevices().then((r) => {
      if (r.ok && r.data?.code === 200) setDevices(r.data.data || []);
    });
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const params = { limit: 1000 };
      if (deviceId) params.deviceId = deviceId;
      if (range?.[0]) params.start = range[0].startOf("day").unix();
      if (range?.[1]) params.end = range[1].endOf("day").unix();
      const r = await listEnv(params);
      if (r.ok && r.data?.code === 200) {
        setRows(r.data.data || []);
      } else {
        message.error(r.data?.message || "加载环境数据失败");
      }
    } finally {
      setLoading(false);
    }
  }, [deviceId, range, message]);

  useEffect(() => {
    load();
  }, [load]);

  const deviceMap = useMemo(
    () => Object.fromEntries(devices.map((d) => [d.device_id, d])),
    [devices],
  );

  const columns = [
    {
      title: "时间",
      dataIndex: "ts",
      width: 170,
      render: (v) => dayjs.unix(v).format("YYYY-MM-DD HH:mm:ss"),
    },
    {
      title: "设备",
      dataIndex: "device_id",
      width: 220,
      render: (v) => {
        const d = deviceMap[v];
        return (
          <Space size={6}>
            {d?.name || v}
            <Tag style={{ margin: 0 }}>{v}</Tag>
          </Space>
        );
      },
    },
    {
      title: "摘要",
      dataIndex: "payload",
      render: (p) => <PayloadSummary payload={p} />,
    },
  ];

  return (
    <div>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          marginBottom: 16,
          gap: 12,
          flexWrap: "wrap",
        }}
      >
        <Space size={12} wrap>
          <Select
            style={{ minWidth: 220 }}
            placeholder="全部设备"
            allowClear
            showSearch
            optionFilterProp="label"
            value={deviceId}
            onChange={setDeviceId}
            options={devices.map((d) => ({
              value: d.device_id,
              label: d.name ? `${d.name} (${d.device_id})` : d.device_id,
            }))}
          />
          <RangePicker
            value={range}
            onChange={setRange}
            allowEmpty={[true, true]}
            presets={[
              { label: "今天", value: [dayjs().startOf("day"), dayjs()] },
              {
                label: "最近 7 天",
                value: [dayjs().subtract(6, "day").startOf("day"), dayjs()],
              },
              {
                label: "最近 30 天",
                value: [dayjs().subtract(29, "day").startOf("day"), dayjs()],
              },
            ]}
          />
        </Space>
        <Space>
          <span style={{ color: "#999", alignSelf: "center" }}>
            共 {rows.length} 条
          </span>
          <Button icon={<ReloadOutlined />} onClick={load}>
            刷新
          </Button>
        </Space>
      </div>
      <Table
        rowKey="id"
        size="middle"
        columns={columns}
        dataSource={rows}
        loading={loading}
        pagination={{ pageSize: 20, showSizeChanger: false, showTotal: (t) => `共 ${t} 条` }}
        expandable={{
          expandedRowRender: (record) => (
            <ExpandedPayload payload={record.payload} />
          ),
          rowExpandable: (record) =>
            record.payload && typeof record.payload === "object",
        }}
      />
    </div>
  );
}
