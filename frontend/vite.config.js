import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// ws:true 的代理连接会常驻,vite 的 SIGINT 处理只调 server.close(),
// 而 http-server 要等所有连接(含已升级的 WebSocket socket)自然关闭才退出,
// 浏览器页面还持有 WS 时 node 进程就挂住退不出。
// 这里跟踪全部 socket(含 upgrade 升级连接),Ctrl+C 时先 destroy 再关服,
// 保证 vite 能干净退出。
function closeSocketsOnExit() {
  return {
    name: "close-sockets-on-exit",
    configureServer(server) {
      const sockets = new Set();
      server.httpServer.on("connection", (s) => {
        sockets.add(s);
        s.on("close", () => sockets.delete(s));
      });
      server.httpServer.on("upgrade", (_req, s) => {
        sockets.add(s);
        s.on("close", () => sockets.delete(s));
      });
      const kill = () => {
        for (const s of sockets) s.destroy();
        server.httpServer.close(() => process.exit(0));
        // 兜底:300ms 后无论如何强制退出
        setTimeout(() => process.exit(0), 300).unref();
      };
      process.on("SIGINT", kill);
      process.on("SIGTERM", kill);
    },
  };
}

export default defineConfig({
  plugins: [react(), closeSocketsOnExit()],
  server: {
    port: 5173,
    proxy: {
      // 所有 /api 请求转发到后端，避免 CORS；ws:true 支持 WebSocket 升级（/api/ws）
      "/api": {
        target: "http://localhost:13884",
        changeOrigin: true,
        ws: true,
        rewrite: (p) => p.replace(/^\/api/, ""),
      },
    },
  },
});
