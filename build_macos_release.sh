#!/usr/bin/env bash
# ==============================================================================
# build_macos_release.sh - Tokage macOS ARM64 (Apple Silicon) 发行版构建脚本
# ==============================================================================

set -euo pipefail

# 颜色输出定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
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

echo -e "${CYAN}======================================================${NC}"
echo -e "${CYAN}        Tokage macOS ARM64 Release Builder           ${NC}"
echo -e "${CYAN}======================================================${NC}"

# 1. 检查操作系统
OS_TYPE="$(uname -s)"
if [ "${OS_TYPE}" != "Darwin" ]; then
  log_error "本脚本专用于 macOS 构建，当前系统为: ${OS_TYPE}"
  exit 1
fi

TARGET_ARCH="aarch64-apple-darwin"
log_info "目标架构: ${BOLD}${TARGET_ARCH}${NC} (Apple Silicon ARM64)"

# 2. 检查基础构建环境 (Rust, Node, 包管理器)
log_info "检查构建工具链..."

if ! command -v node >/dev/null 2>&1; then
  log_error "未找到 Node.js，请先安装 Node.js (推荐 v20+)"
  exit 1
fi
log_success "Node.js 版本: $(node -v)"

if ! command -v cargo >/dev/null 2>&1; then
  log_error "未找到 Rust (cargo)，请先安装 Rust 工具链: https://rustup.rs"
  exit 1
fi
log_success "Rust / Cargo 版本: $(cargo --version)"

# 检查/添加 Rust 目标架构
if ! rustup target list --installed | grep -q "${TARGET_ARCH}"; then
  log_warn "未检测到 Rust 目标架构 ${TARGET_ARCH}，正在尝试自动安装..."
  rustup target add "${TARGET_ARCH}"
  log_success "已添加 ${TARGET_ARCH} 目标架构"
else
  log_success "Rust 目标架构已就绪: ${TARGET_ARCH}"
fi

# 3. 确定前端包管理器
PKG_MANAGER="npm"
if [ -f "pnpm-lock.yaml" ] && command -v pnpm >/dev/null 2>&1; then
  PKG_MANAGER="pnpm"
elif [ -f "yarn.lock" ] && command -v yarn >/dev/null 2>&1; then
  PKG_MANAGER="yarn"
elif [ -f "bun.lockb" ] && command -v bun >/dev/null 2>&1; then
  PKG_MANAGER="bun"
fi
log_info "使用包管理器: ${PKG_MANAGER}"

# 4. 检查与安装前端依赖
if [ ! -d "node_modules" ]; then
  log_warn "未检测到 node_modules，正在安装前端依赖..."
  case "${PKG_MANAGER}" in
    pnpm) pnpm install ;;
    yarn) yarn install ;;
    npm)  npm install ;;
    bun)  bun install ;;
  esac
  log_success "前端依赖安装完成"
fi

# 5. 代码质量检查 (i18n & TypeScript 类型)
log_info "执行代码质量与国际化合规检查..."
if npm run check:i18n; then
  log_success "i18n 检查通过"
else
  log_error "i18n 检查失败，请修正后重新构建"
  exit 1
fi

if npm run typecheck; then
  log_success "TypeScript 类型检查通过"
else
  log_error "TypeScript 类型检查失败，请修正类型错误后重新构建"
  exit 1
fi

# 6. 确定 Tauri CLI 路径
TAURI_CMD=""
if [ -f "./node_modules/.bin/tauri" ]; then
  TAURI_CMD="./node_modules/.bin/tauri"
elif command -v cargo-tauri >/dev/null 2>&1; then
  TAURI_CMD="cargo-tauri"
else
  case "${PKG_MANAGER}" in
    pnpm) TAURI_CMD="pnpm tauri" ;;
    yarn) TAURI_CMD="yarn tauri" ;;
    npm)  TAURI_CMD="npx tauri" ;;
    bun)  TAURI_CMD="bun x tauri" ;;
  esac
fi

# 7. 开始 Release 构建
log_info "开始构建 macOS ARM64 发布版本 (${TARGET_ARCH})..."
echo -e "${CYAN}------------------------------------------------------${NC}"

START_TIME=$(date +%s)

${TAURI_CMD} build --target "${TARGET_ARCH}" "$@"

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

echo -e "${CYAN}------------------------------------------------------${NC}"
log_success "Release 构建完成！耗时: ${ELAPSED} 秒"

# 8. 产物汇总与展示
BUNDLE_DIR="src-tauri/target/${TARGET_ARCH}/release/bundle"
DMG_DIR="${BUNDLE_DIR}/dmg"
MACOS_DIR="${BUNDLE_DIR}/macos"

echo ""
echo -e "${GREEN}${BOLD}======================================================${NC}"
echo -e "${GREEN}${BOLD}                 构建产物清单                         ${NC}"
echo -e "${GREEN}${BOLD}======================================================${NC}"

FOUND_ARTEFACTS=false

if [ -d "${DMG_DIR}" ]; then
  for f in "${DMG_DIR}"/*.dmg; do
    if [ -f "${f}" ]; then
      FOUND_ARTEFACTS=true
      SIZE=$(du -h "${f}" | cut -f1)
      echo -e "${BOLD}DMG 安装包:${NC}"
      echo -e "  - ${CYAN}${f}${NC} (${BOLD}${SIZE}${NC})"
    fi
  done
fi

if [ -d "${MACOS_DIR}" ]; then
  for f in "${MACOS_DIR}"/*.app; do
    if [ -d "${f}" ]; then
      FOUND_ARTEFACTS=true
      SIZE=$(du -sh "${f}" | cut -f1)
      echo -e "\n${BOLD}.app 应用程序:${NC}"
      echo -e "  - ${CYAN}${f}${NC} (${BOLD}${SIZE}${NC})"
    fi
  done
fi

if [ "${FOUND_ARTEFACTS}" = false ]; then
  echo -e "${YELLOW}未在默认 bundle 目录找到 .dmg 或 .app，产物位于:${NC}"
  echo -e "  src-tauri/target/${TARGET_ARCH}/release/"
else
  echo -e "\n${BLUE}💡 提示: 您可以运行以下命令在访达 (Finder) 中打开产物目录:${NC}"
  echo -e "   open \"${BUNDLE_DIR}\"\n"
fi
