#!/usr/bin/env bash
# 本地编译打包 macOS 桌面版。前端构建由 prepare-app 自动触发,无需单独执行。
# 用法:
#   ./desktop/scripts/build-desktop.sh              # 全量:重建 Rust 后端 + 前端 + tauri 打包
#   ./desktop/scripts/build-desktop.sh --skip-rust  # 复用已编译 Rust 二进制,只重打包
# 产物:
#   desktop/src-tauri/target/release/bundle/macos/RetainPDF.app
#   desktop/src-tauri/target/release/bundle/dmg/RetainPDF_*.dmg
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DESKTOP="$ROOT/desktop"
SKIP_RUST=0

for arg in "$@"; do
  case "$arg" in
    --skip-rust) SKIP_RUST=1 ;;
    -h|--help)
      awk 'NR==1 { next } /^#/ { sub(/^# ?/, ""); print; next } { exit }' "$0"
      exit 0
      ;;
    *)
      echo "未知参数: $arg (支持 --skip-rust)" >&2
      exit 1
      ;;
  esac
done

fail() { echo "[build] $*" >&2; exit 1; }

command -v node >/dev/null || fail "缺少 node"
command -v npm >/dev/null || fail "缺少 npm"
command -v cargo >/dev/null || fail "缺少 cargo (Rust 工具链)"
command -v pkg-config >/dev/null || fail "缺少 pkg-config (mupdf-sys 编译依赖,brew install pkg-config)"
[ -d "$ROOT/frontend/node_modules" ] || fail "frontend/node_modules 缺失,先执行: npm --prefix frontend install"
[ -d "$DESKTOP/node_modules" ] || fail "desktop/node_modules 缺失,先执行: npm --prefix desktop install"
[ -x "$DESKTOP/src/runtime/mac/python/bin/python3" ] || fail "随包 Python 缺失: desktop/src/runtime/mac/python"

if [ "$SKIP_RUST" -eq 1 ]; then
  echo "[build] --skip-rust:复用现有 Rust 二进制"
  [ -x "$ROOT/backend/rust_api/target/release/rust_api" ] || fail "rust_api 二进制缺失,去掉 --skip-rust 重建"
  [ -x "$ROOT/backend/rendering_orchestrator/target/release/render_rs" ] || fail "render_rs 二进制缺失,去掉 --skip-rust 重建"
else
  echo "[build] 编译 Rust 后端 (rust_api + render_rs)..."
  (cd "$DESKTOP" && npm run build:rust-api)
  (cd "$DESKTOP" && npm run build:render-rs)
fi

echo "[build] 打包 macOS 桌面版 (前端构建 + prepare-app + prepare-tauri + tauri build)..."
export RETAIN_PDF_BUNDLE_MAC_PYTHON=1
export RETAIN_PDF_DESKTOP_PLATFORM=darwin
# 缺省版本取 desktop/package.json 的 4.1.10;如要覆盖可: export RETAIN_PDF_VERSION=4.1.10
(cd "$DESKTOP" && npm run dist:tauri)

APP="$DESKTOP/src-tauri/target/release/bundle/macos/RetainPDF.app"
DMG_DIR="$DESKTOP/src-tauri/target/release/bundle/dmg"
echo "[build] 完成"
[ -d "$APP" ] && echo "  App: $APP"
for dmg in "$DMG_DIR"/*.dmg; do
  [ -e "$dmg" ] && echo "  DMG: $dmg"
done
