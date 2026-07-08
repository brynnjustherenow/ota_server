# 硬件对接文档（CanMV K230 / MicroPython）

本文档面向**编写设备端代码**的工程师。描述设备如何与服务端通信：订阅 MQTT、拉 OTA、传视频。

---

## 1. 连接信息

| 项 | 值 |
|---|---|
| HTTP Base URL | `http://<server_host>:13884` |
| MQTT Broker | `<server_host>:1883`（明文 MQTT） |
| MQTT Client ID | 自定义，建议 `k230-<device_id>` |
| MQTT 用户名/密码 | 无（默认 broker 允许匿名） |

> `server_host` 与 broker 地址由部署方提供。本文示例用 `http://192.168.1.100:13884`，按实际替换。

---

## 2. MQTT 订阅

### 2.1 OTA 升级通知（必订）

- **Topic**: `k230/cam/cmd`
- **Payload** (JSON, UTF-8):

```json
{"ts": 1720000000000, "version": "1.0.1"}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `ts` | u64 | 服务端时间戳（毫秒） |
| `version` | string | 最新可用版本号 |

**收到后的动作**：与本机当前版本比较；若服务端版本更新，触发 §3 的 OTA 流程。

### 2.2 视频上传事件（按需订）

- **Topic**: `k230/cam/status`
- **Payload**:

```json
{
  "event": "video_uploaded",
  "device_id": "k230-001",
  "filename": "20260707_143022.mp4",
  "ts": 1720000000000
}
```

管理面板/联动服务用，**设备端通常不需要订阅**。除非想让多台设备之间感知彼此的视频。

---

## 3. OTA 升级流程（设备端核心任务）

### 3.1 流程图

```
┌─────────────────────────────────────────────────────────────┐
│ 1. MQTT 收到 {version}，与本机版本比较                       │
└─────────────────────────────────────────────────────────────┘
                            ↓ 需要升级
┌─────────────────────────────────────────────────────────────┐
│ 2. GET /ota/{version}/manifest                              │
│    解析 files 数组：每个文件含 path / name / md5 / size     │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│ 3. 检查本地剩余存储 ≥ sum(file.size)，不够则放弃并告警      │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│ 4. 逐个文件：                                                │
│    GET /ota/{version}/files/{path/name}                     │
│    （上次成功可带 If-None-Match: "<md5>" 触发 304 跳过）    │
│    收到 200 → 边下边写 .new，边下边算 MD5                    │
│    收到 304 → 跳过（本地缓存仍有效）                         │
│    下载完毕比对 MD5；不一致 → 删 .new，整体放弃              │
└─────────────────────────────────────────────────────────────┘
                            ↓ 全部就绪
┌─────────────────────────────────────────────────────────────┐
│ 5. 把所有 .new 重命名为正式名（os.rename 是原子的）         │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│ 6. 写入新版本号到本地配置，machine.reset() 重启              │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 manifest 接口

**请求**
```
GET /ota/{version}/manifest
```

**响应**（`200 OK`，`Content-Type: application/json`）：

```json
{
  "create_at": 1720000000000,
  "version": "1.0.1",
  "file_count": 3,
  "files": [
    {"name": "main.mpy",       "path": "",        "md5": "d41d8cd98f00b204e9800998ecf8427e", "size": 4096},
    {"name": "foo.mpy",        "path": "lib",     "md5": "a1b2c3d4e5f6...",                   "size": 2048},
    {"name": "bar.mpy",        "path": "lib/drv", "md5": "5e6f7a8b9c0d...",                   "size": 1024}
  ]
}
```

| 字段 | 说明 |
|---|---|
| `name` | 文件名（不含目录） |
| `path` | 子目录，根文件为空串 |
| `md5` | 32 字符小写 hex |
| `size` | 字节数 |

**下载 URL 拼装**：path 为空时 `relpath = name`；否则 `relpath = path + "/" + name`。

### 3.3 文件下载接口

**请求**
```
GET /ota/{version}/files/{relpath}
```

`relpath` 中的 `/` 保留，URL 路径段需分别 `urlencode`（如 `lib/foo.mpy` → `lib/foo.mpy`，含特殊字符才需编码）。

**可选请求头**

| 头 | 值 | 作用 |
|---|---|---|
| `If-None-Match` | `"<md5>"` | 服务端比对 ETag，命中返回 304 节省流量 |
| `Range` | `bytes=START-END` | 断点续传（详见 §3.5） |

**响应**

| 状态 | 含义 | 响应头 |
|---|---|---|
| `200 OK` | 全量返回 | `Content-Length`、`Content-Type: application/octet-stream`、`ETag: "<md5>"`（若在 manifest 中）、`Accept-Ranges: bytes` |
| `206 Partial Content` | Range 命中 | `Content-Range: bytes START-END/TOTAL`、`Content-Length: <chunk>`、`ETag`、`Accept-Ranges: bytes` |
| `304 Not Modified` | If-None-Match 命中 | `ETag`，无 body |
| `404` | 版本或文件不存在 | 业务错误 JSON |
| `400` | 路径非法（含 `..`、`\`、绝对路径） | 业务错误 JSON |

### 3.4 MicroPython 完整示例

```python
import urequests as requests
import uhashlib, ujson, os, machine, time

BASE = "http://192.168.1.100:13884"
STORAGE = "/sd"                     # 实际存储路径
CUR_VER_FILE = STORAGE + "/.version"

def current_version():
    try:
        with open(CUR_VER_FILE) as f:
            return f.read().strip()
    except:
        return "0.0.0"

def version_gt(a, b):
    """a > b 时返回 True，简单按点分段比较"""
    pa = [int(x) for x in a.split(".")]
    pb = [int(x) for x in b.split(".")]
    return pa > pb

def download_file(version, rel, expected_md5):
    """下载单个文件到 .new，返回 True/False"""
    url = f"{BASE}/ota/{version}/files/{rel}"
    basename = rel.split("/")[-1]
    dst = f"{STORAGE}/{basename}.new"

    r = requests.get(url)
    if r.status_code != 200:
        print(f"[ota] {rel} HTTP {r.status_code}")
        return False

    h = uhashlib.md5()
    with open(dst, "wb") as fp:
        while True:
            chunk = r.raw.read(1024)
            if not chunk:
                break
            fp.write(chunk)
            h.update(chunk)

    actual = "".join("%02x" % b for b in h.digest())
    if actual != expected_md5:
        print(f"[ota] md5 mismatch {rel}: {actual} != {expected_md5}")
        os.remove(dst)
        return False
    return True

def do_ota(new_version):
    print(f"[ota] start upgrade to {new_version}")
    # 1. 拉清单
    r = requests.get(f"{BASE}/ota/{new_version}/manifest")
    if r.status_code != 200:
        print(f"[ota] manifest HTTP {r.status_code}")
        return False
    meta = ujson.loads(r.text)

    # 2. 容量预检
    total = sum(f["size"] for f in meta["files"])
    # statvfs 返回 (frsize, bsize, blocks, bfree, ...)
    stat = os.statvfs(STORAGE)
    free = stat[0] * stat[3]
    if free < total * 1.1:
        print(f"[ota] insufficient storage: need {total}, free {free}")
        return False

    # 3. 逐文件下载到 .new
    downloaded = []
    for f in meta["files"]:
        rel = f["path"] + "/" + f["name"] if f["path"] else f["name"]
        if not download_file(new_version, rel, f["md5"]):
            # 清理已下载的 .new
            for d in downloaded:
                try: os.remove(d)
                except: pass
            return False
        downloaded.append(f"{STORAGE}/{f['name']}.new")

    # 4. 全部就绪，原子切换
    for f in meta["files"]:
        os.rename(f"{STORAGE}/{f['name']}.new", f"{STORAGE}/{f['name']}")

    # 5. 记录新版本号并重启
    with open(CUR_VER_FILE, "w") as fp:
        fp.write(new_version)
    print(f"[ota] done, rebooting...")
    time.sleep(0.5)
    machine.reset()
```

### 3.5 断点续传（可选，弱网/大文件场景）

服务端支持 HTTP Range。如需断点续传：

```python
# 询问服务端已有多少
def resume_from(rel):
    # 先 HEAD 或带 Range: bytes=0-0 取 Content-Range
    r = requests.get(f"{BASE}/.../files/{rel}",
                     headers={"Range": "bytes=0-0"})
    # Content-Range: bytes 0-0/12345 → 总大小 12345
    cr = r.headers.get("Content-Range", "bytes 0-0/0")
    total = int(cr.split("/")[-1])

    # 检查本地 .new 已写多少
    local_size = os.stat(dst)[6] if exists(dst) else 0
    if local_size >= total:
        return total  # 已下完

    # 续传
    r = requests.get(url, headers={"Range": f"bytes={local_size}-"})
    with open(dst, "ab") as fp:        # 追加
        while True:
            chunk = r.raw.read(1024)
            if not chunk: break
            fp.write(chunk)
```

### 3.6 用 If-None-Match 跳过未变文件

如果版本之间文件没变化（md5 一致），可避免重复下载：

```python
def fetch_if_needed(version, rel, local_md5):
    headers = {}
    if local_md5:
        headers["If-None-Match"] = f'"{local_md5}"'
    r = requests.get(f"{BASE}/ota/{version}/files/{rel}", headers=headers)
    if r.status_code == 304:
        return None          # 本地缓存仍有效
    if r.status_code != 200:
        raise Exception(f"HTTP {r.status_code}")
    return r.content         # 新内容
```

设备端需持久化每个文件的 md5（可写到本地 `.md5cache`），重启后读出来用作 If-None-Match。

---

## 4. 视频上传流程

### 4.1 接口

**请求**
```
POST /video/{device_id}
Content-Type: video/mp4            # 可选，建议正确填写
Content-Disposition: attachment; filename="<filename>.mp4"
Content-Length: <bytes>

<raw video bytes>
```

- `device_id` 写在 URL 路径里（每台设备一个 ID）
- `filename` 通过 `Content-Disposition` 头传递；服务端会按这个名字保存
- **body 是原始字节流**，不是 multipart
- 缺省 `Content-Disposition` 时，服务端按 `{时间戳}.mp4` 命名

**响应**（`200 OK`）

```json
{
  "code": 200,
  "message": "success",
  "data": {
    "device_id": "k230-001",
    "filename": "clip_001.mp4",
    "size": 10485760,
    "md5": "d41d8cd98f00b204e9800998ecf8427e"
  }
}
```

**错误响应**

| 状态 | 含义 |
|---|---|
| `400` | 非法 device_id 或 filename（含 `..`、`\` 等） |
| `500` | 磁盘满 / IO 错误 |

### 4.2 关键点

- 服务端**已关闭 2MB body 限制**，可直接传大视频
- 上传完成后服务端会向 `k230/cam/status` 广播 `video_uploaded` 事件
- 同名文件会被**覆盖**，无版本保留；要存历史请在 filename 里加时间戳
- 服务端计算 MD5 并返回，设备端可与自己算的对比以确认完整性

### 4.3 MicroPython 上传示例

```python
import urequests as requests, uhashlib, time

BASE = "http://192.168.1.100:13884"
DEVICE_ID = "k230-001"

def upload_video(filepath, custom_name=None):
    filename = custom_name or filepath.split("/")[-1]
    with open(filepath, "rb") as f:
        data = f.read()

    # 边读边算 md5（可选，用于和服务端核对）
    h = uhashlib.md5()
    h.update(data)
    local_md5 = "".join("%02x" % b for b in h.digest())

    url = f"{BASE}/video/{DEVICE_ID}"
    headers = {
        "Content-Type": "video/mp4",
        "Content-Disposition": f'attachment; filename="{filename}"',
    }
    r = requests.post(url, data=data, headers=headers)

    if r.status_code != 200:
        print(f"upload failed: HTTP {r.status_code} {r.text}")
        return False

    resp = r.json()
    if resp.get("code") != 200:
        print(f"upload failed: {resp}")
        return False

    info = resp["data"]
    if info["md5"] != local_md5:
        print(f"warning: md5 mismatch local={local_md5} server={info['md5']}")

    print(f"upload OK: {info['filename']} ({info['size']} bytes)")
    return True

# 用法：按时间戳自动命名，避免覆盖
upload_video("/sd/clip.mp4",
             custom_name=f"clip_{time.strftime('%Y%m%d_%H%M%S')}.mp4")
```

### 4.4 大文件分块读取（RAM 紧张时）

K230 RAM 有限，几 GB 视频不能一次性读到内存。改用流式 POST：

```python
import usocket, ujson

def upload_stream(filepath, filename, chunk=8 * 1024):
    size = os.stat(filepath)[6]
    sock = usocket.socket()
    addr = usocket.getaddrinfo("192.168.1.100", 13884)[0][-1]
    sock.connect(addr)

    # 手写 HTTP 请求头
    req = (
        f"POST /video/{DEVICE_ID} HTTP/1.1\r\n"
        f"Host: 192.168.1.100:13884\r\n"
        f"Content-Type: video/mp4\r\n"
        f'Content-Disposition: attachment; filename="{filename}"\r\n'
        f"Content-Length: {size}\r\n"
        f"Connection: close\r\n\r\n"
    )
    sock.send(req.encode())

    # 流式读盘 → 流式发送
    with open(filepath, "rb") as f:
        while True:
            buf = f.read(chunk)
            if not buf:
                break
            sock.send(buf)

    # 读响应
    resp = b""
    while True:
        b = sock.recv(1024)
        if not b: break
        resp += b
    sock.close()

    # 解析 body
    body = resp.split(b"\r\n\r\n", 1)[1]
    return ujson.loads(body)
```

---

## 5. device_id 命名约定

| 规则 | 说明 |
|---|---|
| 长度 | 1-64 字符 |
| 允许字符 | 字母、数字、`-`、`_` |
| 禁止 | `/`、`\`、`..`、空串 |
| 建议 | `k230-<MAC 末 4 位>` 或 `k230-<序列号>`，如 `k230-a1b2` |

device_id 在 URL 中**不要 urlencode 特殊字符**，仅用允许的字符集即可。

---

## 6. 错误处理与重试建议

| 场景 | 设备端策略 |
|---|---|
| HTTP 超时 | 指数退避重试 3 次：5s → 10s → 20s |
| HTTP 5xx | 同上 |
| HTTP 4xx | 不重试，记录日志并放弃（请求本身有问题） |
| MD5 不一致 | 删本地 `.new`，整体回滚，下次 OTA 周期再试 |
| 磁盘满 | 上报状态到 `status_topic`（自定义事件），放弃本次 |
| MQTT 断连 | 重连后重新订阅 `cmd_topic`；主动 GET 一次最新 manifest 检查是否错过通知 |

---

## 7. 调试小贴士

1. **本地手动测 OTA**：浏览器访问 `http://192.168.1.100:5173`（前端管理页）→ "OTA Manifest" 输入版本号可看清单结构
2. **测下载**：`curl -v http://192.168.1.100:13884/ota/1.0.1/files/main.mpy -o test.bin`
3. **测 Range**：`curl -v -H "Range: bytes=0-99" http://.../files/main.mpy -o part.bin`
4. **测上传**：`curl -X POST http://192.168.1.100:13884/video/k230-001 -H 'Content-Disposition: attachment; filename="t.mp4"' --data-binary @test.mp4`
5. **看 MQTT 流**：用 MQTTX 订阅 `k230/cam/#` 观察所有消息
