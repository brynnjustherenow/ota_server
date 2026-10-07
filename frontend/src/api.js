// 所有接口封装。BASE 走 Vite 代理（见 vite.config.js）。
// 统一注入 Authorization: Bearer <token>；401 统一交给 onUnauthorized 处理（登出+跳登录页）。
export const API_BASE_URL = import.meta.env.VITE_API_BASE_URL;
const BASE = API_BASE_URL;

// ─────────────── 登录态存储 ───────────────

const TOKEN_KEY = "ota_token";

export const tokenStore = {
  get: () => localStorage.getItem(TOKEN_KEY),
  set: (t) => localStorage.setItem(TOKEN_KEY, t),
  clear: () => localStorage.removeItem(TOKEN_KEY),
};

let onUnauthorized = null;

/**
 * 注册 401 统一处理（AuthProvider 里设置：清 token → 跳 /login）
 * @param {() => void} fn
 */
export function setUnauthorizedHandler(fn) {
  onUnauthorized = fn;
}

function authHeaders(extra) {
  const h = { ...(extra || {}) };
  const t = tokenStore.get();
  if (t) h["Authorization"] = `Bearer ${t}`;
  return h;
}

async function parseBody(resp) {
  const text = await resp.text();
  let data = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = text;
  }
  return { ok: resp.ok, status: resp.status, data, headers: resp.headers };
}

/**
 * 统一请求入口：注入 token、JSON 序列化、401 兜底
 * @param {string} path
 * @param {{method?:string, body?:any, headers?:object}} [opts]
 */
async function request(path, opts = {}) {
  const { method = "GET", body, headers } = opts;
  const h = authHeaders(headers);
  if (body !== undefined && !(body instanceof FormData) && !h["Content-Type"]) {
    h["Content-Type"] = "application/json";
  }
  const resp = await fetch(`${BASE}${path}`, {
    method,
    headers: h,
    body:
      body !== undefined && !(body instanceof FormData)
        ? JSON.stringify(body)
        : body,
  });
  // 登录接口自身返回 401（密码错误），不触发全局登出
  if (resp.status === 401 && onUnauthorized && !path.startsWith("/auth/login")) {
    onUnauthorized();
  }
  return parseBody(resp);
}

function headersFromObj(obj) {
  const h = new Headers();
  Object.entries(obj).forEach(([k, v]) => v != null && h.append(k, v));
  return h;
}

/**
 * WebSocket 地址：/ws?token=...
 * dev（/api）→ ws://<当前host>/api/ws（vite 代理升级）；
 * 生产（http(s)://...）→ ws(s)://.../ws
 */
export function wsUrl() {
  const t = tokenStore.get();
  const base = API_BASE_URL || "";
  let url;
  if (base.startsWith("http://") || base.startsWith("https://")) {
    url = base.replace(/^http/, "ws") + "/ws";
  } else {
    const proto = window.location.protocol === "https:" ? "wss" : "ws";
    url = `${proto}://${window.location.host}${base}/ws`;
  }
  return `${url}?token=${encodeURIComponent(t || "")}`;
}

function xhrTokenHeader() {
  const t = tokenStore.get();
  return t ? { Authorization: `Bearer ${t}` } : {};
}

/**
 * 简单版本号比较（按点分段整数比较，非严格 SemVer）
 * @returns {number} 1 if a>b, -1 if a<b, 0 if equal
 */
export function compareVersions(a, b) {
  const pa = String(a || "")
    .split(".")
    .map((n) => parseInt(n, 10) || 0);
  const pb = String(b || "")
    .split(".")
    .map((n) => parseInt(n, 10) || 0);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i++) {
    const va = pa[i] || 0;
    const vb = pb[i] || 0;
    if (va > vb) return 1;
    if (va < vb) return -1;
  }
  return 0;
}

// ─────────────── 认证 / 个人信息 ───────────────

/**
 * 登录
 * @returns {{ok, status, data}} data.data = { token, user }
 */
export async function login(username, password) {
  return request("/auth/login", {
    method: "POST",
    body: { username, password },
  });
}

/** 当前登录用户信息 */
export async function getMe() {
  return request("/auth/me");
}

/** 修改自己的密码（需原密码），成功返回新 token */
export async function changeMyPassword(oldPassword, newPassword) {
  return request("/auth/password", {
    method: "PUT",
    body: { old_password: oldPassword, new_password: newPassword },
  });
}

/** 修改自己的昵称 */
export async function updateMyProfile(nickname) {
  return request("/auth/profile", { method: "PUT", body: { nickname } });
}

// ─────────────── 用户管理（admin+） ───────────────

/** 用户列表（super_admin 全部 / admin 仅组用户） */
export async function listUsers() {
  return request("/users");
}

/**
 * 创建用户
 * @param {{username:string, password:string, nickname?:string, role:string}} payload
 */
export async function createUser(payload) {
  return request("/users", { method: "POST", body: payload });
}

/**
 * 编辑用户（昵称/角色/启用状态均可选）
 * @param {number|string} id
 * @param {{nickname?:string, role?:string, enabled?:boolean}} payload
 */
export async function updateUser(id, payload) {
  return request(`/users/${id}`, { method: "PUT", body: payload });
}

/** 管理员重置用户密码 */
export async function resetUserPassword(id, newPassword) {
  return request(`/users/${id}/password`, {
    method: "PUT",
    body: { new_password: newPassword },
  });
}

/** 删除用户 */
export async function deleteUser(id) {
  return request(`/users/${id}`, { method: "DELETE" });
}

// ─────────────── 设备管理 ───────────────

/**
 * 设备概览列表（组用户自动只返回名下设备）
 * @returns {{ok, status, data}} data.data = [{device_id,name,owner_user_id,owner_nickname,owner_username,last_active_at}]
 */
export async function getDevices() {
  return request("/devices");
}

/**
 * 修改设备展示名（仅前端展示，不同步硬件；组用户仅限名下设备）
 * @param {string} deviceId
 * @param {string} name
 */
export async function setDeviceName(deviceId, name) {
  return request(`/devices/${encodeURIComponent(deviceId)}`, {
    method: "PUT",
    body: { name },
  });
}

/**
 * 分配/解除设备归属（admin+）
 * @param {string} deviceId
 * @param {number|null} userId 组用户 id；null = 解除归属
 */
export async function setDeviceOwner(deviceId, userId) {
  return request(`/devices/${encodeURIComponent(deviceId)}/owner`, {
    method: "PUT",
    body: { user_id: userId },
  });
}

// ─────────────── 环境数据 ───────────────

/**
 * 查询环境数据(组用户自动限定名下设备)
 * @param {{deviceId?:string, start?:number, end?:number, limit?:number}} [params]
 * start/end 为 unix 秒(含端点),结果按 ts 倒序
 */
export async function listEnv(params = {}) {
  const qs = new URLSearchParams();
  if (params.deviceId) qs.set("device_id", params.deviceId);
  if (params.start != null) qs.set("start", String(params.start));
  if (params.end != null) qs.set("end", String(params.end));
  if (params.limit != null) qs.set("limit", String(params.limit));
  const s = qs.toString();
  return request(`/env${s ? `?${s}` : ""}`);
}

// ─────────────── 视频 ───────────────

/**
 * 上传视频
 * @param {string} deviceId
 * @param {File} file
 * @param {string} filename  自定义文件名（可选，缺省用 file.name）
 * @param {(pct:number)=>void} onProgress
 */
export async function uploadVideo(deviceId, file, filename, onProgress) {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open("POST", `${BASE}/video/${encodeURIComponent(deviceId)}`);
    xhr.setRequestHeader(
      "Content-Disposition",
      `attachment; filename="${filename || file.name}"`,
    );
    Object.entries(xhrTokenHeader()).forEach(([k, v]) =>
      xhr.setRequestHeader(k, v),
    );
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable && onProgress) onProgress(e.loaded / e.total);
    };
    xhr.onload = () => {
      let data = null;
      try {
        data = xhr.responseText ? JSON.parse(xhr.responseText) : null;
      } catch {
        data = xhr.responseText;
      }
      if (xhr.status >= 200 && xhr.status < 300) {
        resolve({ ok: true, status: xhr.status, data });
      } else {
        resolve({ ok: false, status: xhr.status, data });
      }
    };
    xhr.onerror = () => reject(new Error("网络错误"));
    xhr.send(file);
  });
}

/**
 * 列出所有上传过视频的 device_id
 */
export async function listDevices() {
  return request("/video");
}

/**
 * 列出设备视频
 */
export async function listVideos(deviceId) {
  return request(`/video/${encodeURIComponent(deviceId)}`);
}

/**
 * 下载视频，浏览器触发保存
 */
export async function downloadVideo(deviceId, filename) {
  const resp = await fetch(
    `${BASE}/video/${encodeURIComponent(deviceId)}/${encodeURIComponent(filename)}`,
    { headers: authHeaders() },
  );
  if (!resp.ok) return { ok: false, status: resp.status };
  const blob = await resp.blob();
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
  return { ok: true, status: resp.status };
}

// ─────────────── OTA ───────────────

/**
 * 发布 OTA：多文件 multipart 上传，每个 part 的 filename 可包含子路径如 `lib/foo.mpy`
 * @param {string} version
 * @param {Array<{filename:string, file:File}>} items
 * @param {(pct:number)=>void} onProgress
 */
export async function publishOta(version, items, onProgress) {
  return new Promise((resolve, reject) => {
    const form = new FormData();
    items.forEach((it, i) => {
      // multipart field 的 filename 设为相对路径，服务端会自动建子目录
      form.append(`file${i}`, it.file, it.filename);
    });

    const xhr = new XMLHttpRequest();
    xhr.open("POST", `${BASE}/ota/${encodeURIComponent(version)}/publish`);
    Object.entries(xhrTokenHeader()).forEach(([k, v]) =>
      xhr.setRequestHeader(k, v),
    );
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable && onProgress) onProgress(e.loaded / e.total);
    };
    xhr.onload = () => {
      let data = null;
      try {
        data = xhr.responseText ? JSON.parse(xhr.responseText) : null;
      } catch {
        data = xhr.responseText;
      }
      if (xhr.status >= 200 && xhr.status < 300)
        resolve({ ok: true, status: xhr.status, data });
      else resolve({ ok: false, status: xhr.status, data });
    };
    xhr.onerror = () => reject(new Error("网络错误"));
    xhr.send(form);
  });
}

/**
 * 列出所有已发布版本号（按 SemVer 倒序，最新在前）
 */
export async function listVersions() {
  return request("/ota");
}

/**
 * 拉取 manifest
 */
export async function getManifest(version) {
  return request(`/ota/${encodeURIComponent(version)}/manifest`);
}

/**
 * 下载 OTA 单个文件，演示 ETag / If-None-Match
 * @param {string} version
 * @param {string} relPath  如 "lib/foo.mpy"
 * @param {string|null} etag 上次返回的 ETag，命中则服务端返回 304
 */
export async function downloadOtaFile(version, relPath, etag = null) {
  const resp = await fetch(
    `${BASE}/ota/${encodeURIComponent(version)}/files/${relPath
      .split("/")
      .map(encodeURIComponent)
      .join("/")}`,
    { headers: authHeaders({ "If-None-Match": etag }) },
  );
  if (resp.status === 304) {
    return {
      ok: true,
      status: 304,
      notModified: true,
      etag: resp.headers.get("etag"),
    };
  }
  if (!resp.ok) return parseBody(resp);
  const blob = await resp.blob();
  const newEtag = resp.headers.get("etag");
  const blobUrl = URL.createObjectURL(blob);
  return {
    ok: true,
    status: 200,
    notModified: false,
    etag: newEtag,
    blobUrl,
    size: blob.size,
  };
}

/**
 * 触发 MQTT 广播
 * @param {string} version
 * @param {string} [deviceId]  可选；非空时 payload 带 device_id，仅匹配设备应用
 * @param {boolean} [carryDeviceConfig]  仅当 deviceId 非空时有意义：
 *   true=携带设备专属 config（无则退回全局）；false=忽略设备 config 退回全局。默认 true。
 */
export async function notifyOta(version, deviceId, carryDeviceConfig) {
  const params = new URLSearchParams();
  const did = deviceId && deviceId.trim();
  if (did) params.set("device_id", did);
  if (did && carryDeviceConfig !== undefined) {
    params.set("carry_device_config", String(carryDeviceConfig));
  }
  const qs = params.toString();
  return request(
    `/ota/${encodeURIComponent(version)}/notify${qs ? `?${qs}` : ""}`,
    { method: "POST" },
  );
}

/**
 * 读取某版本的 config（随 fleet_update 下发 merge 到设备 device_cfg.json）
 */
export async function getVersionConfig(version) {
  return request(`/ota/${encodeURIComponent(version)}/config`);
}

/**
 * 设置某版本的 config
 * @param {string} version
 * @param {object} configObj  会原样存入 sqlite，下发时 deep_merge 进设备配置
 */
export async function setVersionConfig(version, configObj) {
  return request(`/ota/${encodeURIComponent(version)}/config`, {
    method: "POST",
    body: { config: configObj },
  });
}

// ─────────────── 设备 config 模板 ───────────────

/**
 * 拉取设备 config 模板（来自项目根 device_cfg.json）
 */
export async function getConfigTemplate() {
  return request("/ota/template");
}

// ─────────────── 设备专属 config ───────────────

/**
 * 列出某版本下已配置过的 device_id
 */
export async function listDeviceConfigs(version) {
  return request(`/ota/${encodeURIComponent(version)}/devices`);
}

/**
 * 读取某版本下某设备的专属 config
 */
export async function getDeviceConfig(version, deviceId) {
  return request(
    `/ota/${encodeURIComponent(version)}/config/${encodeURIComponent(deviceId)}`,
  );
}

/**
 * 保存某版本下某设备的专属 config（不广播）
 */
export async function setDeviceConfig(version, deviceId, configObj) {
  return request(
    `/ota/${encodeURIComponent(version)}/config/${encodeURIComponent(deviceId)}`,
    { method: "POST", body: { config: configObj } },
  );
}

/**
 * 删除某版本下某设备的专属 config
 */
export async function deleteDeviceConfig(version, deviceId) {
  return request(
    `/ota/${encodeURIComponent(version)}/config/${encodeURIComponent(deviceId)}`,
    { method: "DELETE" },
  );
}

/**
 * 发布（保存 + 广播）某版本下某设备的 config。
 * 服务端会下发带 device_id 的 fleet_update，仅匹配设备应用。
 */
export async function publishDeviceConfig(version, deviceId, configObj) {
  return request(
    `/ota/${encodeURIComponent(version)}/publish_device/${encodeURIComponent(deviceId)}`,
    { method: "POST", body: { config: configObj } },
  );
}
