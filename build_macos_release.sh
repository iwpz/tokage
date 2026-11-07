#!/usr/bin/env bash
# ==============================================================================
# build_macos_release.sh - Tokage macOS ARM64 (Apple Silicon) 发行版构建脚本
# 支持自动加载 macos.signing.env（或复用 ../fish-idle/client/macos.signing.env）
# 进行 Developer ID 签名与 Apple 公证 (Notarization + Stapling)。
#
# 参数：
#   (默认)        若检测到签名与公证凭证，自动执行 Developer ID 签名 + Apple 公证
#   --notarize    强制要求 Developer ID 签名 + Apple 公证（缺凭证时报错退出）
#   --sign-only   仅执行 Developer ID 签名（不提交 Apple 公证，适合本地快速出包）
#   --no-sign     跳过开发者证书，使用 Ad-hoc 签名
#   --no-open     构建完成后不自动弹出 Finder 窗口
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

# 解析脚本参数（其余参数透传给 tauri build）
SIGN_MODE="auto" # auto | notarize | sign-only | no-sign
OPEN_FINDER=1
TAURI_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --notarize)
      SIGN_MODE="notarize"
      shift
      ;;
    --sign-only)
      SIGN_MODE="sign-only"
      shift
      ;;
    --no-sign)
      SIGN_MODE="no-sign"
      shift
      ;;
    --no-open)
      OPEN_FINDER=0
      shift
      ;;
    *)
      TAURI_ARGS+=("$1")
      shift
      ;;
  esac
done

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

# 2. 加载 macOS 签名与公证配置
load_signing_env() {
  local env_file=""
  if [ -f "${SCRIPT_DIR}/macos.signing.env" ]; then
    env_file="${SCRIPT_DIR}/macos.signing.env"
  elif [ -f "${SCRIPT_DIR}/../fish-idle/client/macos.signing.env" ]; then
    env_file="${SCRIPT_DIR}/../fish-idle/client/macos.signing.env"
  fi

  if [ -n "${env_file}" ]; then
    log_info "加载签名凭证配置: ${env_file}"
    while IFS= read -r line || [ -n "${line}" ]; do
      local trimmed
      trimmed="$(echo "${line}" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
      if [ -z "${trimmed}" ] || [[ "${trimmed}" == \#* ]]; then
        continue
      fi
      if [[ "${trimmed}" == *=* ]]; then
        local key="${trimmed%%=*}"
        local val="${trimmed#*=}"
        key="$(echo "${key}" | tr -d '[:space:]')"
        val="$(echo "${val}" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' -e 's/^"//' -e 's/"$//' -e "s/^'//" -e "s/'$//")"
        if [ -z "${!key:-}" ]; then
          export "${key}=${val}"
        fi
      fi
    done < "${env_file}"
  fi

  # 兼容 fish-idle 的变量名 (CSC_NAME / APPLE_APP_SPECIFIC_PASSWORD) 与 Tauri 原生变量名
  if [ -z "${APPLE_SIGNING_IDENTITY:-}" ] && [ -n "${CSC_NAME:-}" ]; then
    export APPLE_SIGNING_IDENTITY="${CSC_NAME}"
  fi
  if [ -z "${APPLE_PASSWORD:-}" ]; then
    if [ -n "${APPLE_APP_SPECIFIC_PASSWORD:-}" ]; then
      export APPLE_PASSWORD="${APPLE_APP_SPECIFIC_PASSWORD}"
    elif [ -n "${APPLE_ID_PASSWORD:-}" ]; then
      export APPLE_PASSWORD="${APPLE_ID_PASSWORD}"
    fi
  fi
}

SHIM_DIR=""
cleanup_shim() {
  if [ -n "${SHIM_DIR}" ] && [ -d "${SHIM_DIR}" ]; then
    rm -rf "${SHIM_DIR}"
  fi
}
trap cleanup_shim EXIT

# 安装 codesign 包装层：签名前自动清理 Documents/iCloud 下的 xattr 扩展属性，并在 Apple TSA 抖动时自动重试
install_codesign_shim() {
  SHIM_DIR="$(mktemp -d -t tokage-codesign-shim)"
  cat > "${SHIM_DIR}/codesign" <<'SHIM_EOF'
#!/usr/bin/env bash
REAL="/usr/bin/codesign"
MAX=8
DELAY=2
attempt=1
tmp="$(mktemp -t tokage-codesign-out)"
cleanup() { rm -f "$tmp"; }
trap cleanup EXIT

for arg in "$@"; do
  if [[ -e "$arg" ]]; then
    /usr/bin/xattr -cr "$arg" >/dev/null 2>&1 || true
    /usr/bin/chflags -R nouchg,noschg "$arg" >/dev/null 2>&1 || true
  fi
done

while (( attempt <= MAX )); do
  set +e
  "$REAL" "$@" >"$tmp" 2>&1
  rc=$?
  set -e
  if (( rc == 0 )); then
    cat "$tmp"
    exit 0
  fi
  cat "$tmp" >&2
  if ! grep -Eq "timestamp service is not available|Operation not permitted|resource fork, Finder information, or similar detritus" "$tmp"; then
    exit "$rc"
  fi
  if (( attempt == MAX )); then
    exit "$rc"
  fi
  echo "[codesign-retry] 签名暂遇 TSA/xattr 抖动，${DELAY}s 后重试 ($attempt/$MAX)..." >&2
  for arg in "$@"; do
    if [[ -e "$arg" ]]; then
      /usr/bin/xattr -cr "$arg" >/dev/null 2>&1 || true
    fi
  done
  sleep "$DELAY"
  attempt=$((attempt + 1))
done
exit 1
SHIM_EOF
  chmod +x "${SHIM_DIR}/codesign"
  export PATH="${SHIM_DIR}:${PATH}"
  log_info "已启用 codesign 扩展属性清理与 Apple TSA 自动重试包装"
}

ACTIVE_SIGNING=0
ACTIVE_NOTARIZE=0

if [ "${SIGN_MODE}" = "no-sign" ]; then
  unset APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID CSC_NAME APPLE_APP_SPECIFIC_PASSWORD
  export APPLE_SIGNING_IDENTITY="-"
  log_warn "签名模式: Ad-hoc 签名 (--no-sign)，跳过开发者证书与 Apple 公证"
else
  load_signing_env
  if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
    ACTIVE_SIGNING=1
    log_success "已启用 Developer ID 签名身份: ${APPLE_SIGNING_IDENTITY}"
    install_codesign_shim
  elif [ "${SIGN_MODE}" = "notarize" ] || [ "${SIGN_MODE}" = "sign-only" ]; then
    log_error "未找到 APPLE_SIGNING_IDENTITY (或 CSC_NAME)，请配置 macos.signing.env"
    exit 1
  else
    log_warn "未检测到 Developer ID 签名身份，将回退为 Ad-hoc 签名"
  fi

  if [ "${SIGN_MODE}" = "sign-only" ]; then
    unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID
    log_info "公证模式: 仅签名 (--sign-only)，跳过 Apple 公证"
  elif [ "${ACTIVE_SIGNING}" -eq 1 ]; then
    if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
      ACTIVE_NOTARIZE=1
      log_success "已启用 Apple 公证 (Team ID: ${APPLE_TEAM_ID}, Apple ID: ${APPLE_ID})"
    elif [ "${SIGN_MODE}" = "notarize" ]; then
      log_error "缺少 Apple 公证凭证 (APPLE_ID / APPLE_APP_SPECIFIC_PASSWORD / APPLE_TEAM_ID)"
      exit 1
    else
      unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID
      log_warn "未完整配置公证凭证，本次仅执行 Developer ID 签名"
    fi
  fi
fi

# 3. 检查基础构建环境 (Rust, Node, 包管理器)
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

# 4. 确定前端包管理器
PKG_MANAGER="npm"
if [ -f "pnpm-lock.yaml" ] && command -v pnpm >/dev/null 2>&1; then
  PKG_MANAGER="pnpm"
elif [ -f "yarn.lock" ] && command -v yarn >/dev/null 2>&1; then
  PKG_MANAGER="yarn"
elif [ -f "bun.lockb" ] && command -v bun >/dev/null 2>&1; then
  PKG_MANAGER="bun"
fi
log_info "使用包管理器: ${PKG_MANAGER}"

# 5. 检查与安装前端依赖
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

# 6. 代码质量检查 (i18n & TypeScript 类型)
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

# 7. 确定 Tauri CLI 路径
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

# 清理旧的 bundle 产物，防止旧版本号 .dmg 混入输出清单
BUNDLE_DIR="src-tauri/target/${TARGET_ARCH}/release/bundle"
rm -rf "${BUNDLE_DIR}/dmg" "${BUNDLE_DIR}/macos"

# 8. 开始 Release 构建
log_info "开始构建 macOS ARM64 发布版本 (${TARGET_ARCH})..."
echo -e "${CYAN}------------------------------------------------------${NC}"

START_TIME=$(date +%s)

if [ ${#TAURI_ARGS[@]} -gt 0 ]; then
  ${TAURI_CMD} build --target "${TARGET_ARCH}" "${TAURI_ARGS[@]}"
else
  ${TAURI_CMD} build --target "${TARGET_ARCH}"
fi

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

echo -e "${CYAN}------------------------------------------------------${NC}"
log_success "Release 构建完成！耗时: ${ELAPSED} 秒"

# 9. 产物签名/公证自检与汇总展示
DMG_DIR="${BUNDLE_DIR}/dmg"
MACOS_DIR="${BUNDLE_DIR}/macos"

if [ "${ACTIVE_SIGNING}" -eq 1 ] && [ -d "${MACOS_DIR}" ]; then
  for app in "${MACOS_DIR}"/*.app; do
    if [ -d "${app}" ]; then
      log_info "校验 .app 签名完整性: ${app}"
      /usr/bin/codesign --verify --deep --strict --verbose=2 "${app}"
      log_success ".app 签名验证通过"
      if [ "${ACTIVE_NOTARIZE}" -eq 1 ]; then
        log_info "校验 .app Gatekeeper / 公证票据..."
        xcrun stapler validate "${app}" || true
        spctl --assess --type execute -vv "${app}" || true
      fi
    fi
  done
fi

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
  if [ "${OPEN_FINDER}" -eq 1 ] && [ -d "src-tauri/target/${TARGET_ARCH}/release" ]; then
    log_info "正在访达 (Finder) 中打开构建产物目录..."
    open "src-tauri/target/${TARGET_ARCH}/release"
  fi
else
  TARGET_OPEN_DIR="${DMG_DIR}"
  if [ ! -d "${TARGET_OPEN_DIR}" ]; then
    TARGET_OPEN_DIR="${BUNDLE_DIR}"
  fi
  if [ "${OPEN_FINDER}" -eq 1 ]; then
    log_info "正在访达 (Finder) 中打开产物目录: ${TARGET_OPEN_DIR}"
    open "${TARGET_OPEN_DIR}"
  fi
fi
echo -e "${GREEN}🎉 全部完成！${NC}\n"
