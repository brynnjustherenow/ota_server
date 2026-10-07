# OTA Server 本机开发任务
#
# 常用:
#   just dev            # 同时启动前后端(默认)
#   just stop           # 强杀残留的前后端进程(按端口)
#   just backend        # 仅后端
#   just frontend       # 仅前端
# 后端 http://localhost:13884;前端 http://localhost:5173(/api 由 vite 代理到后端)

# 默认:同时启动前后端
default: dev

# 同时启动:后端 + 前端,Ctrl+C 一并退出
# 注意:不直接用 cargo run——cargo 会把 exe 放进新进程组,Windows 的 Ctrl+C
# 控制台事件送达不了它,cargo 被中断后 exe 会变孤儿。这里先 build 再直接跑
# exe,它与控制台同组,能收到 Ctrl+C。
dev:
    cargo build
    ./target/debug/ota_server.exe &
    npm --prefix frontend run dev &
    wait

# 仅后端(axum,端口来自 .conf.toml,默认 13884)
# 同上理由:build 后直接跑 exe,保证 Ctrl+C 可停
backend:
    cargo build
    ./target/debug/ota_server.exe

# 仅前端(vite dev,默认 5173)
frontend:
    npm --prefix frontend run dev

# 强杀残留的前后端进程(调试中断异常时兜底,按端口找 PID)
stop:
    @for port in 13884 5173; do \
        pid=$(netstat -ano | grep ":$port" | grep LISTENING | awk '{print $NF}' | head -1); \
        if [ -n "$pid" ]; then echo "kill :$port (pid $pid)"; taskkill //F //PID $pid || true; fi; \
    done

# 前端生产构建 → frontend/dist
build-frontend:
    npm --prefix frontend run build

# 后端编译检查(不启动)
check:
    cargo check

# 后端 clippy
clippy:
    cargo clippy
