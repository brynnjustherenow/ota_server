// 所有接口封装。BASE 走 Vite 代理（见 vite.config.js）。
const BASE = "/api";

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

function headersFromObj(obj) {
  const h = new Headers();
  Object.entries(obj).forEach(([k, v]) => v != null && h.append(k, v));
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
      `attachment; filename="${filename || file.name}"`
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
  const resp = await fetch(`${BASE}/video`);
  return parseBody(resp);
}

/**
 * 列出设备视频
 */
export async function listVideos(deviceId) {
  const resp = await fetch(`${BASE}/video/${encodeURIComponent(deviceId)}`);
  return parseBody(resp);
}

/**
 * 下载视频，浏览器触发保存
 */
export async function downloadVideo(deviceId, filename) {
  const resp = await fetch(
    `${BASE}/video/${encodeURIComponent(deviceId)}/${encodeURIComponent(filename)}`
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
      if (xhr.status >= 200 && xhr.status < 300) resolve({ ok: true, status: xhr.status, data });
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
  const resp = await fetch(`${BASE}/ota`);
  return parseBody(resp);
}

/**
 * 拉取 manifest
 */
export async function getManifest(version) {
  const resp = await fetch(
    `${BASE}/ota/${encodeURIComponent(version)}/manifest`
  );
  return parseBody(resp);
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
    { headers: headersFromObj({ "If-None-Match": etag }) }
  );
  if (resp.status === 304) {
    return { ok: true, status: 304, notModified: true, etag: resp.headers.get("etag") };
  }
  if (!resp.ok) return parseBody(resp);
  const blob = await resp.blob();
  const newEtag = resp.headers.get("etag");
  const blobUrl = URL.createObjectURL(blob);
  return { ok: true, status: 200, notModified: false, etag: newEtag, blobUrl, size: blob.size };
}

/**
 * 触发 MQTT 广播
 */
export async function notifyOta(version) {
  const resp = await fetch(
    `${BASE}/ota/${encodeURIComponent(version)}/notify`,
    { method: "POST" }
  );
  return parseBody(resp);
}
