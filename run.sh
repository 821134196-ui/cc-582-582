#!/usr/bin/env bash
# 一条命令启动摄影棚灯光排期系统（后端 Axum + 前端静态资源）
set -euo pipefail
cd "$(dirname "$0")"

source "$HOME/.cargo/env" 2>/dev/null || true

PORT="${PORT:-8080}"
export PORT

# 1) 前端：依赖安装 + 构建到 dist/（后端直接托管）
if [ ! -d frontend/node_modules ]; then
  echo "[run] 安装前端依赖…"
  (cd frontend && npm install)
fi
if [ ! -f dist/index.html ] || [ "${REBUILD_FRONTEND:-0}" = "1" ]; then
  echo "[run] 构建前端…"
  (cd frontend && npm run build)
fi

# 2) 后端：编译并运行（首次会执行 SQLite 迁移与演示数据播种）
echo "[run] 启动后端 http://localhost:${PORT} …"
exec cargo run
