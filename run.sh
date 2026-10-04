#!/usr/bin/env bash
# 一条本地命令启动整个系统：
#   1. 构建 Svelte 前端（产物到 frontend/dist）
#   2. 编译并启动 Rust/Axum 后端（托管前端 + /api，默认 8080 端口）
# 用法：./run.sh        （开发模式，前端改动可另开 `cd frontend && npm run dev`，代理到 8080）
set -euo pipefail
cd "$(dirname "$0")"

PORT="${PORT:-8080}"

# 1. Rust 工具链
if ! command -v cargo >/dev/null 2>&1; then
  if [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo ">> 未检测到 Rust，使用 rustup 安装…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  source "$HOME/.cargo/env"
fi

# 2. 前端依赖与构建
if [ ! -d frontend/node_modules ]; then
  echo ">> 安装前端依赖…"
  (cd frontend && npm install)
fi
if [ "${SKIP_FRONTEND_BUILD:-0}" != "1" ]; then
  echo ">> 构建前端…"
  (cd frontend && npm run build)
fi

# 3. 编译后端
echo ">> 编译后端…"
(cd backend && cargo build)

# 4. 启动（数据库自动建表+种子；前端静态文件由后端直接托管）
mkdir -p backend/data
echo ">> 启动服务： http://localhost:${PORT}"
cd backend
DATABASE_URL="${DATABASE_URL:-sqlite:data/studio.db?mode=rwc}" \
FRONTEND_DIST="${FRONTEND_DIST:-../frontend/dist}" \
PORT="${PORT}" \
exec ./target/debug/studio-schedule
