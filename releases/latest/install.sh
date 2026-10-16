#!/usr/bin/env bash
# O Muse 一键安装脚本（公测阶段，ad-hoc 签名分发）
#
# 用法：
#   快速安装：   curl -fsSL <base>/releases/latest/install.sh | bash
#   可审计安装： curl -fsSL <base>/releases/latest/install.sh -o install.sh && less install.sh && bash install.sh
#
# 参数：
#   --version <v>        安装指定版本（默认 latest）
#   --install-dir <dir>  自定义安装目录（默认 ~/Applications）
#   --no-launch          安装后不启动
#   --system             安装到 /Applications（需要 sudo）
#   --help               显示帮助
#
# 兼容性：macOS 系统自带 bash 3.2（不使用 mapfile / 关联数组 / ${var^^}）。
set -euo pipefail

APP_NAME="O Muse"
# BUNDLE_ID 由 tauri.conf.json 单源管理，此处不重复定义。

# 分发基址：默认 GitHub Releases，可用环境变量覆盖（便于迁移到 CDN）。
BASE_URL="${OMUSE_BASE_URL:-https://xsin.github.io/o-muse/releases/latest}"

# 可被 --version 覆盖
VERSION="latest"
# 由 VERSION.txt 解析出的真实版本号（--version latest 时使用）
RESOLVED_VERSION=""
DOWNLOAD_URL=""
INSTALL_ROOT="${HOME}/Applications"
DO_LAUNCH=1
USE_SUDO=0

# 日志一律走 stderr：脚本的 stdout 会被 $(...) 捕获用于传值，
# 若 log 也写 stdout 会污染返回值（曾导致"已获取空包名"后继续执行）。
log()  { printf '%s[install]%s %s\n' "$(printf '\033[1;34m')" "$(printf '\033[0m')" "$*" >&2; }
ok()   { printf '%s[  ok  ]%s %s\n' "$(printf '\033[1;32m')" "$(printf '\033[0m')" "$*" >&2; }
warn() { printf '%s[ warn ]%s %s\n' "$(printf '\033[1;33m')" "$(printf '\033[0m')" "$*" >&2; }
die()  { printf '%s[ fail ]%s %s\n' "$(printf '\033[1;31m')" "$(printf '\033[0m')" "$*" >&2; exit 1; }

# 文件名含空格（"O Muse"），URL 必须编码。
urlencode() {
  python3 -c 'import sys,urllib.parse; print(urllib.parse.quote(sys.argv[1]))' "$1"
}

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
}

# ---- 参数解析 ------------------------------------------------------------
while [ $# -gt 0 ]; do
  case "$1" in
    --version)      VERSION="${2:?--version 需要一个值}"; shift 2 ;;
    --install-dir)  INSTALL_ROOT="${2:?--install-dir 需要一个值}"; shift 2 ;;
    --no-launch)    DO_LAUNCH=0; shift ;;
    --system)       USE_SUDO=1; shift ;;
    -h|--help)      usage ;;
    *)              die "未知参数：$1（--help 查看用法）" ;;
  esac
done

[ "$USE_SUDO" -eq 1 ] && INSTALL_ROOT="/Applications"

# ---- 架构检测 ------------------------------------------------------------
ARCH_RAW="$(uname -m)"
case "$ARCH_RAW" in
  arm64)  ARCH="arm64" ;;
  x86_64) ARCH="x86_64" ;;
  *)      die "不支持的架构：$ARCH_RAW" ;;
esac

log "应用：${APP_NAME}"
log "架构：${ARCH}"
log "版本：${VERSION}"

# ---- 临时目录与清理 ------------------------------------------------------
TMP_DIR="$(mktemp -d)"
cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT INT TERM

# ---- 下载地址 ------------------------------------------------------------
# 产物按**真实版本号**命名（O Muse-<version>-<arch>.zip），而 latest 只是
# 一个"指向最新版本"的标签。默认（--version latest）时先从
# VERSION.txt 解析真实版本号，再按版本号取产物。
#
# 先把 BASE_URL 归一化成 **releases 根**：无论用户传
#   .../releases  或  .../releases/latest  或  .../releases/latest/download
# 都归一到 .../releases，避免出现 .../latest/latest/... 这类拼接错误。
RELEASES_ROOT="$BASE_URL"
RELEASES_ROOT="${RELEASES_ROOT%/latest/download}"
RELEASES_ROOT="${RELEASES_ROOT%/latest}"
RELEASES_ROOT="${RELEASES_ROOT%/}"

# latest 解析：两种托管的"最新版"入口路径不同，都试一次
#   Pages/自建： <root>/latest/VERSION.txt
#   GitHub    ： <root>/latest/download/VERSION.txt（GitHub 的 latest 资产别名）
if [ "$VERSION" = "latest" ]; then
  for vt in "${RELEASES_ROOT}/latest/VERSION.txt" \
            "${RELEASES_ROOT}/latest/download/VERSION.txt"; do
    # 网络失败不得让脚本退出（set -e 会继承命令替换的失败状态），故 `|| true`
    RESOLVED_VERSION="$(curl -fsL --retry 3 --retry-delay 2 -sS "$vt" 2>/dev/null \
      | tr -d '[:space:]' || true)"
    [ -n "$RESOLVED_VERSION" ] && break
  done
  if [ -n "$RESOLVED_VERSION" ]; then
    log "解析到最新版本：${RESOLVED_VERSION}"
  else
    log "未找到 VERSION.txt，按 latest 标签直接取产物"
  fi
fi

# 候选下载基址。两种托管的路径布局不同，全部列出、按序尝试、命中即用：
#   Pages/自建    ： <root>/<version>/<file>
#   GitHub Release： <root>/download/<tag>/<file>   ← 注意是 download 在前
#   GitHub tag 常带 v 前缀（v0.1.0），而版本号是 0.1.0，故两种都列。
build_base_candidates() {
  local v="$1"
  local out="$RELEASES_ROOT/$v"
  if [ -n "$v" ]; then
    out="$out $RELEASES_ROOT/download/$v"
    case "$v" in
      v*) out="$out $RELEASES_ROOT/download/${v#v}" ;;
      *)  out="$out $RELEASES_ROOT/download/v$v" ;;
    esac
  fi
  printf '%s' "$out"
}

if [ -n "$RESOLVED_VERSION" ]; then
  BASE_CANDIDATES="$(build_base_candidates "$RESOLVED_VERSION")"
elif [ "$VERSION" = "latest" ]; then
  BASE_CANDIDATES="${RELEASES_ROOT}/latest ${RELEASES_ROOT}/latest/download"
else
  BASE_CANDIDATES="$(build_base_candidates "$VERSION")"
fi

# 尝试下载单个 URL；成功（且内容确为 zip）返回 0 并把路径写入 $ZIP_PATH。
#
# 以**落盘内容的魔术字节**判定真伪，而不是 curl 的退出码：
# GitHub Pages 对缺失文件会返回 "HTTP 404 + HTML body"，经代理链路时
# 甚至表现为连接重置（curl 退出 56）而非 -f 期望的 22，状态码不可信。
try_download() {
  local url="$1"
  local out="$2"

  if ! curl -fL --retry 3 --retry-delay 2 -sS -o "$out" "$url" 2>/dev/null; then
    rm -f "$out"
    return 1
  fi
  [ -s "$out" ] || { rm -f "$out"; return 1; }
  # zip 本地文件头魔术字节为 "PK"（PK\x03\x04）
  [ "$(head -c 2 "$out" 2>/dev/null)" = "PK" ] || { rm -f "$out"; return 1; }
  return 0
}

# 下载指定架构的包；成功则把本地路径写到 $ZIP_PATH 并返回 0。
# 注意：成功与否用返回码传递，日志不参与（全部 stderr）。
download_candidate() {
  local want="$1"
  # 产物文件名用**不带 v 前缀**的版本号：GitHub tag 常写 v0.1.0，
  # 而构建产出的文件名是 O Muse-0.1.0-<arch>.zip。
  local ver_for_name
  if [ -n "$RESOLVED_VERSION" ]; then
    ver_for_name="${RESOLVED_VERSION#v}"
  else
    ver_for_name="${VERSION#v}"
  fi

  local zipname
  if [ "$ver_for_name" = "latest" ]; then
    zipname="${APP_NAME}-latest-${want}.zip"
  else
    zipname="${APP_NAME}-${ver_for_name}-${want}.zip"
  fi

  # GitHub Releases 会把 asset 名中的空格替换为点
  # （"O Muse-1.0-arm64.zip" → "O.Muse-1.0-arm64.zip"），
  # 而 gh-pages / 自建托管保留空格。注意**不能**用空格做词分割来遍历
  # 变体——文件名本身含空格会被拆碎，故这里显式逐个尝试。
  local zipname_dot
  zipname_dot="$(printf '%s' "$zipname" | tr ' ' '.')"

  local out="$TMP_DIR/$zipname"

  # 候选基址（GitHub /download/ 风格 + 纯版本目录）× 文件名变体
  local base
  for base in $BASE_CANDIDATES; do
    if try_download "${base}/$(urlencode "$zipname")" "$out"; then
      log "已下载：${zipname} ← ${base}"
      ZIP_PATH="$out"; DOWNLOAD_URL="${base}/$(urlencode "$zipname")"
      return 0
    fi
    if [ "$zipname_dot" != "$zipname" ]; then
      if try_download "${base}/$(urlencode "$zipname_dot")" "$out"; then
        log "已下载：${zipname_dot} ← ${base}（Releases 命名变体）"
        ZIP_PATH="$out"; DOWNLOAD_URL="${base}/$(urlencode "$zipname_dot")"
        return 0
      fi
    fi
  done
  return 1
}

ZIP_PATH=""
SELECTED_ARCH=""
for want in universal "$ARCH"; do
  if download_candidate "$want"; then
    SELECTED_ARCH="$want"
    break
  fi
  warn "未找到 ${want} 包，尝试下一个"
done

[ -n "$ZIP_PATH" ] || die "无法下载任何可用包（架构 ${ARCH}），请检查版本号或网络"
ok "已获取 ${SELECTED_ARCH} 包：$(basename "$ZIP_PATH")"

# ---- SHA256 校验 ---------------------------------------------------------
# 优先从 checksums.txt 找对应行，回退到 <file>.sha256。
# checksums.txt 用 shasum 的双列格式（"<hash>  <filename>"）。
# 匹配时把空格与点视为等价，以兼容 GitHub Releases 对 asset 名的
# 空格→点替换（checksums 内容仍写原始名，asset 名可能已被改写）。
BASE_NAME="$(basename "$ZIP_PATH")"
BASE_NAME_NORM="$(printf '%s' "$BASE_NAME" | tr ' .' '--')"
EXPECTED=""
CHECKSUMS_URL="$(dirname "$DOWNLOAD_URL")/checksums.txt"
if curl -fL --retry 3 --retry-delay 2 -sS -o "$TMP_DIR/checksums.txt" "$CHECKSUMS_URL" 2>/dev/null; then
  EXPECTED="$(awk -v want="$BASE_NAME_NORM" '
    { f=$NF; gsub(/[ .]/, "-", f); if (f==want) { print $1; exit } }
  ' "$TMP_DIR/checksums.txt")"
fi
if [ -z "$EXPECTED" ]; then
  # <name>.sha256 也逐个变体尝试（同样不能用空格做词分割）
  BASE_NAME_DOT="$(printf '%s' "$BASE_NAME" | tr ' ' '.')"
  for try_name in "$BASE_NAME" "$BASE_NAME_DOT"; do
    SHA_URL="$(dirname "$DOWNLOAD_URL")/$(urlencode "$try_name").sha256"
    if curl -fL --retry 3 --retry-delay 2 -sS -o "$TMP_DIR/pkg.sha256" "$SHA_URL" 2>/dev/null; then
      EXPECTED="$(awk '{print $1}' "$TMP_DIR/pkg.sha256" | head -n1)"
      [ -n "$EXPECTED" ] && break
    fi
  done
fi

ACTUAL="$(shasum -a 256 "$ZIP_PATH" | awk '{print $1}')"
if [ -z "$EXPECTED" ]; then
  # 无校验文件时不静默放行：明确告知并要求 --no-verify 显式跳过
  if [ "${OMUSE_SKIP_VERIFY:-0}" = "1" ]; then
    warn "未找到 SHA256 校验文件，已按 OMUSE_SKIP_VERIFY=1 跳过校验"
    warn "实际 SHA256：${ACTUAL}"
  else
    die "未找到 SHA256 校验文件，拒绝安装（可用 OMUSE_SKIP_VERIFY=1 显式跳过）"
  fi
else
  if [ "$EXPECTED" = "$ACTUAL" ]; then
    ok "SHA256 校验通过：${ACTUAL}"
  else
    die "SHA256 校验失败
  期望：${EXPECTED}
  实际：${ACTUAL}
请重新下载或报告该版本文件损坏。"
  fi
fi

# ---- 解压 ---------------------------------------------------------------
EXTRACT_DIR="$TMP_DIR/extract"
mkdir -p "$EXTRACT_DIR"
log "解压到临时目录"
ditto -x -k "$ZIP_PATH" "$EXTRACT_DIR"

APP_SRC="$EXTRACT_DIR/${APP_NAME}.app"
[ -d "$APP_SRC" ] || die "包内未找到 ${APP_NAME}.app（实际内容：$(find "$EXTRACT_DIR" -mindepth 1 -maxdepth 1 | tr '\n' ' ')）"

# ---- 退出正在运行的旧版本 -------------------------------------------------
if pgrep -x "$APP_NAME" >/dev/null 2>&1; then
  log "检测到正在运行的 ${APP_NAME}，正在退出"
  pkill -x "$APP_NAME" || true
  # 等进程真正退出，避免覆盖时文件被占用
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    pgrep -x "$APP_NAME" >/dev/null 2>&1 || break
    sleep 0.3
  done
fi

# ---- 安装 ---------------------------------------------------------------
mkdir -p "$INSTALL_ROOT"
APP_DEST="$INSTALL_ROOT/${APP_NAME}.app"

log "安装位置：${APP_DEST}"
if [ -e "$APP_DEST" ]; then
  # 覆盖更新：先备份旧版，成功后再删除，避免中途失败导致无应用可用
  BACKUP="$TMP_DIR/${APP_NAME}.app.bak"
  mv "$APP_DEST" "$BACKUP" 2>/dev/null || rm -rf "$APP_DEST"
fi

# sudo 目录（/Applications）可能非当前用户可写
if [ "$USE_SUDO" -eq 1 ] && [ ! -w "$INSTALL_ROOT" ]; then
  log "需要 sudo 写入 ${INSTALL_ROOT}"
  sudo mkdir -p "$INSTALL_ROOT"
  sudo ditto "$APP_SRC" "$APP_DEST"
  sudo chown -R "$(id -u):$(id -g)" "$APP_DEST" 2>/dev/null || true
else
  ditto "$APP_SRC" "$APP_DEST"
fi
ok "已安装到 ${APP_DEST}"

# ---- 防御性移除隔离属性 ---------------------------------------------------
# zip 分发通常不带 quarantine，但用户从浏览器下载 dmg/zip 再手动跑脚本时可能残留
xattr -dr com.apple.quarantine "$APP_DEST" 2>/dev/null || true
ok "已清理 com.apple.quarantine（如存在）"

# ---- 权限与启动 ---------------------------------------------------------
MAIN_BIN="$APP_DEST/Contents/MacOS/${APP_NAME}"
[ -x "$MAIN_BIN" ] || MAIN_BIN="$(find "$APP_DEST/Contents/MacOS" -type f 2>/dev/null | head -n1)"
if [ -n "$MAIN_BIN" ] && [ -e "$MAIN_BIN" ]; then
  chmod +x "$MAIN_BIN" 2>/dev/null || true
fi

log "安装完成"
log "  版本：${VERSION}"
log "  架构：${SELECTED_ARCH}"
log "  路径：${APP_DEST}"
log "  SHA256：${ACTUAL}"

if [ "$DO_LAUNCH" -eq 1 ]; then
  log "启动 ${APP_NAME}"
  open "$APP_DEST" || warn "启动失败，可手动打开：${APP_DEST}"
else
  log "已跳过启动（--no-launch）"
fi

ok "完成"
