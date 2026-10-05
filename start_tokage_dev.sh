#!/usr/bin/env bash
# ==============================================================================
# .start_tokage_dev.sh - Tokage 开发模式启动脚本 (带平台与依赖检测)
# ==============================================================================

set -euo pipefail

# 颜色输出定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

log_info() {
  echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
  echo -e "${GREEN}[OK]${NC} $*"
}

log_warn() {
  echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
  echo -e "${RED}[ERROR]${NC} $*"
}

# 确保在项目根目录运行
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

echo -e "${CYAN}==============================================${NC}"
echo -e "${CYAN}         Tokage Dev Launcher                 ${NC}"
echo -e "${CYAN}==============================================${NC}"

# 1. 平台检测 (OS & Architecture)
OS_TYPE="$(uname -s)"
ARCH_TYPE="$(uname -m)"

log_info "检测运行平台..."
case "${OS_TYPE}" in
  Darwin*)
    PLATFORM="macOS"
    log_success "操作系统: macOS (${ARCH_TYPE})"
    ;;
  Linux*)
    PLATFORM="Linux"
    log_success "操作系统: Linux (${ARCH_TYPE})"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    PLATFORM="Windows"
    log_success "操作系统: Windows (${ARCH_TYPE})"
    ;;
  *)
    PLATFORM="Unknown (${OS_TYPE})"
    log_warn "未识别的操作系统: ${OS_TYPE} (${ARCH_TYPE})"
    ;;
esac

# 2. 检查 Rust / Cargo 环境
log_info "检查 Rust 环境..."
if ! command -v cargo >/dev/null 2>&1; then
  # 尝试加载常见的 cargo 环境路径
  if [ -f "$HOME/.cargo/env" ]; then
    log_info "正在加载 ~/.cargo/env..."
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
  fi
fi

if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
  RUST_VER="$(rustc --version)"
  log_success "Rust 就绪: ${RUST_VER}"
else
  log_error "未检测到 Rust 工具链 (cargo / rustc)！"
  echo "请先安装 Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi

# 3. 检查 Node.js / 包管理器
log_info "检查 Node.js 环境与包管理器..."
if ! command -v node >/dev/null 2>&1; then
  log_error "未检测到 Node.js，请先安装 Node.js (>= 18)！"
  exit 1
else
  NODE_VER="$(node --version)"
  log_success "Node.js 就绪: ${NODE_VER}"
fi

PKG_MANAGER=""
if command -v pnpm >/dev/null 2>&1; then
  PKG_MANAGER="pnpm"
elif command -v yarn >/dev/null 2>&1; then
  PKG_MANAGER="yarn"
elif command -v npm >/dev/null 2>&1; then
  PKG_MANAGER="npm"
elif command -v bun >/dev/null 2>&1; then
  PKG_MANAGER="bun"
else
  log_error "未检测到可用的包管理器 (npm / pnpm / yarn / bun)！"
  exit 1
fi
log_success "使用包管理器: ${PKG_MANAGER}"

# 4. 平台特定依赖提示 (例如 Linux 下 WebKit2GTK 等 Tauri 前置条件)
if [ "${PLATFORM}" = "Linux" ]; then
  log_info "Linux 平台依赖检查提示:"
  # 检查 pkg-config 是否存在
  if ! command -v pkg-config >/dev/null 2>&1; then
    log_warn "未检测到 pkg-config，如果编译失败请安装基本构建工具 (如 build-essential, pkg-config, libwebkit2gtk 等)"
  fi
elif [ "${PLATFORM}" = "macOS" ]; then
  if ! xcode-select -p >/dev/null 2>&1; then
    log_warn "未检测到 Xcode Command Line Tools，编译可能会遇到问题。建议运行: xcode-select --install"
  fi
fi

# 5. 检查 node_modules
if [ ! -d "node_modules" ]; then
  log_warn "未检测到 node_modules，准备自动安装依赖..."
  case "${PKG_MANAGER}" in
    pnpm) pnpm install ;;
    yarn) yarn install ;;
    npm)  npm install ;;
    bun)  bun install ;;
  esac
  log_success "前端依赖安装完成！"
fi

# 6. 确定启动方式
# 检查是否安装了本地或全局 tauri cli
TAURI_EXEC=""
if [ -f "./node_modules/.bin/tauri" ]; then
  TAURI_EXEC="./node_modules/.bin/tauri"
elif command -v cargo-tauri >/dev/null 2>&1; then
  TAURI_EXEC="cargo-tauri"
else
  case "${PKG_MANAGER}" in
    pnpm) TAURI_EXEC="pnpm tauri" ;;
    yarn) TAURI_EXEC="yarn tauri" ;;
    npm)  TAURI_EXEC="npx tauri" ;;
    bun)  TAURI_EXEC="bun x tauri" ;;
  esac
fi

# 7. 启动 Tokage 开发者模式
log_info "正在启动 Tokage 开发模式 (${TAURI_EXEC} dev)..."
echo -e "${CYAN}----------------------------------------------${NC}"
exec ${TAURI_EXEC} dev "$@"
