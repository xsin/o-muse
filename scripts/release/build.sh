#!/usr/bin/env bash
# O Muse 公测期打包脚本（ad-hoc 签名，无 Developer ID / 无公证）
#
# 用法：
#   scripts/release/build.sh [--arch arm64|x86_64|universal] [--out-dir <dir>] [--skip-build]
#
# 产出（默认写入 releases/<version>/）：
#   <AppName>-<version>-<arch>.zip        # 主分发格式（ditto 保留元数据）
#   <AppName>-<version>-<arch>.zip.sha256
#   <AppName>-<version>-<arch>.dmg        # 可选，供手动下载
#   <AppName>-<version>-<arch>.dmg.sha256
#   checksums.txt
#
# 未来切换 Developer ID（不改脚本结构，只改环境变量）：
#   SIGN_IDENTITY="Developer ID Application: …"   # 当前默认 "-"（ad-hoc）
#   NOTARIZE=1 APPLE_ID=… TEAM_ID=… APP_SPECIFIC_PASSWORD=… scripts/release/build.sh
#
# 兼容 macOS bash 3.2。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=./lib/version.sh
# shellcheck source=./lib/version.sh disable=SC1091
source "$SCRIPT_DIR/lib/version.sh"

VERSION="$(read_version)"
APP_NAME="$(read_product_name)"
BUNDLE_ID="$(read_identifier)"

# ---- 参数 ---------------------------------------------------------------
ARCH=""
OUT_DIR=""
SKIP_BUILD=0
while [ $# -gt 0 ]; do
  case "$1" in
    --arch)      ARCH="${2:?}"; shift 2 ;;
    --out-dir)   OUT_DIR="${2:?}"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h|--help)   sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "未知参数：$1" >&2; exit 1 ;;
  esac
done

# ---- 未来签名/公证预留 ---------------------------------------------------
SIGN_IDENTITY="${SIGN_IDENTITY:--}"     # "-" = ad-hoc；将来填 Developer ID
NOTARIZE="${NOTARIZE:-0}"               # 0 = 不公证；将来 1
APPLE_ID="${APPLE_ID:-}"
TEAM_ID="${TEAM_ID:-}"
APP_SPECIFIC_PASSWORD="${APP_SPECIFIC_PASSWORD:-}"

OUT_DIR="${OUT_DIR:-$REPO_ROOT/releases/$VERSION}"

# ---- 目标架构判定 --------------------------------------------------------
HOST_ARCH="$(uname -m)"
case "$HOST_ARCH" in
  arm64)  HOST_TRIPLE_ARCH="aarch64-apple-darwin" ;;
  x86_64) HOST_TRIPLE_ARCH="x86_64-apple-darwin" ;;
  *) echo "不支持的主机架构：$HOST_ARCH" >&2; exit 1 ;;
esac

if [ -z "$ARCH" ]; then
  # 两个 target 都装了才做 universal，否则退化为单架构
  if rustup target list --installed 2>/dev/null | grep -q "aarch64-apple-darwin" \
     && rustup target list --installed 2>/dev/null | grep -q "x86_64-apple-darwin"; then
    ARCH="universal"
  else
    ARCH="$HOST_TRIPLE_ARCH"
    echo "[build] 未安装全部 target，退化为单架构包：$ARCH" >&2
  fi
fi

# 规范架构名：universal / arm64 / x86_64
case "$ARCH" in
  universal)                   ARCH_LABEL="universal"; RUST_TARGETS="aarch64-apple-darwin x86_64-apple-darwin" ;;
  arm64|aarch64-apple-darwin)  ARCH_LABEL="arm64";    RUST_TARGETS="aarch64-apple-darwin" ;;
  x86_64|x86_64-apple-darwin)  ARCH_LABEL="x86_64";   RUST_TARGETS="x86_64-apple-darwin" ;;
  *) echo "未知架构：$ARCH" >&2; exit 1 ;;
esac

echo "[build] 应用：${APP_NAME}"
echo "[build] 版本：${VERSION}"
echo "[build] bundle id：${BUNDLE_ID}"
echo "[build] 架构：${ARCH_LABEL}"
echo "[build] 签名身份：${SIGN_IDENTITY}（\"-\" = ad-hoc）"
echo "[build] 公证：${NOTARIZE}"
echo "[build] 输出：${OUT_DIR}"

mkdir -p "$OUT_DIR"

# ---- 1. 构建 .app -------------------------------------------------------
APP_BUNDLE=""
if [ "$SKIP_BUILD" -eq 0 ]; then
  echo "[build] 安装前端依赖"
  (cd "$REPO_ROOT" && npm install --silent)

  if [ "$ARCH_LABEL" = "universal" ]; then
    echo "[build] 构建 universal（lipo 合并两个架构）"
    for t in $RUST_TARGETS; do
      echo "[build] 目标：$t"
      (cd "$REPO_ROOT" && CARGO_HOME="${CARGO_HOME:-$REPO_ROOT/.cargo-home}" \
        npm_config_cache="${npm_config_cache:-$REPO_ROOT/.npm-cache}" \
        node_modules/.bin/tauri build --target "$t" --no-bundle --ci)
    done
    # 取 arm64 产物为基准，把主可执行 lipo 成 universal
    ARM_APP="$(find "$REPO_ROOT/src-tauri/target/aarch64-apple-darwin/release" -maxdepth 1 -name "*.app" | head -n1)"
    X86_APP="$(find "$REPO_ROOT/src-tauri/target/x86_64-apple-darwin/release" -maxdepth 1 -name "*.app" | head -n1)"
    [ -n "$ARM_APP" ] || { echo "未找到 arm64 .app" >&2; exit 1; }
    APP_BUNDLE="$OUT_DIR/stage-universal"
    rm -rf "$APP_BUNDLE"
    mkdir -p "$APP_BUNDLE"
    ditto "$ARM_APP" "$APP_BUNDLE/${APP_NAME}.app"
    if [ -n "$X86_APP" ]; then
      find "$APP_BUNDLE/${APP_NAME}.app/Contents/MacOS" -type f 2>/dev/null | while read -r arm_bin; do
        rel="${arm_bin#"$APP_BUNDLE/${APP_NAME}".app/}"
        x86_bin="$X86_APP/Contents/${rel#MacOS/}"
        if [ -f "$x86_bin" ]; then
          lipo -create "$arm_bin" "$x86_bin" -output "$arm_bin.tmp" && mv "$arm_bin.tmp" "$arm_bin"
          echo "merged: $(basename "$arm_bin")"
        fi
      done
      echo "[build] lipo 合并完成"
    else
      echo "[build] 警告：未找到 x86_64 .app，universal 包将只含 arm64" >&2
    fi
  else
    echo "[build] 构建单架构：${ARCH_LABEL}"
    (cd "$REPO_ROOT" && CARGO_HOME="${CARGO_HOME:-$REPO_ROOT/.cargo-home}" \
      npm_config_cache="${npm_config_cache:-$REPO_ROOT/.npm-cache}" \
      node_modules/.bin/tauri build --target "$RUST_TARGETS" --no-bundle --ci)
    APP_BUNDLE="$(find "$REPO_ROOT/src-tauri/target/$RUST_TARGETS/release" -maxdepth 1 -name "*.app" | head -n1)"
    [ -n "$APP_BUNDLE" ] || { echo "未找到 .app 产物" >&2; exit 1; }
  fi
else
  echo "[build] 跳过构建（--skip-build），查找既有产物"
  APP_BUNDLE="$(find "$REPO_ROOT/src-tauri/target" -maxdepth 4 -name "*.app" 2>/dev/null | head -n1)"
  [ -n "$APP_BUNDLE" ] || { echo "未找到既有 .app，请先构建" >&2; exit 1; }
fi

# universal 分支下 APP_BUNDLE 指向暂存目录；统一取 .app 路径
if [ -d "$APP_BUNDLE" ]; then
  case "$APP_BUNDLE" in
    *.app) APP_SRC="$APP_BUNDLE" ;;
    *)     APP_SRC="$(find "$APP_BUNDLE" -maxdepth 1 -name "*.app" | head -n1)" ;;
  esac
else
  echo "构建未产出 .app" >&2; exit 1
fi
[ -d "$APP_SRC" ] || { echo "未找到 .app：$APP_SRC" >&2; exit 1; }

# ---- 2. ad-hoc 签名（由内到外，不依赖 --deep）---------------------------
echo "[sign] 签名身份：${SIGN_IDENTITY}"
SIGN_ARGS=(--force --sign "$SIGN_IDENTITY")
# 将来启用 hardened runtime 时加 --options runtime
[ "${ENABLE_HARDENED_RUNTIME:-0}" = "1" ] && SIGN_ARGS+=(--options runtime)

sign_nested() {
  local target="$1"
  # 1) 最深层：Frameworks
  if [ -d "$target/Contents/Frameworks" ]; then
    for fw in "$target/Contents/Frameworks"/*; do
      [ -e "$fw" ] || continue
      echo "[sign]   framework: $(basename "$fw")"
      codesign "${SIGN_ARGS[@]}" "$fw"
    done
  fi
  # 2) Helpers / 嵌套 app
  for nested in "$target/Contents/MacOS"/*.app; do
    [ -e "$nested" ] || continue
    echo "[sign]   嵌套 app: $(basename "$nested")"
    codesign "${SIGN_ARGS[@]}" "$nested"
  done
  # 3) 可执行文件（含 sidecar 注入的 python-stem-backend）
  if [ -d "$target/Contents/MacOS" ]; then
    for exe in "$target/Contents/MacOS"/*; do
      [ -f "$exe" ] || continue
      echo "[sign]   executable: $(basename "$exe")"
      codesign "${SIGN_ARGS[@]}" "$exe"
    done
  fi
  # 4) 外层 bundle 本体
  echo "[sign] bundle: $(basename "$target")"
  codesign "${SIGN_ARGS[@]}" "$target"
}

sign_nested "$APP_SRC"

# ---- 3. 验证签名 --------------------------------------------------------
echo "[verify] codesign --verify --deep --strict --verbose=2"
codesign --verify --deep --strict --verbose=2 "$APP_SRC"
echo "[verify] 签名有效"

# ---- 4. 公证（当前 no-op，将来 NOTARIZE=1）------------------------------
if [ "$NOTARIZE" = "1" ]; then
  [ -n "$APPLE_ID" ] && [ -n "$TEAM_ID" ] && [ -n "$APP_SPECIFIC_PASSWORD" ] || {
    echo "NOTARIZE=1 需要 APPLE_ID / TEAM_ID / APP_SPECIFIC_PASSWORD" >&2; exit 1; }
  ZIP_FOR_NOTARY="$OUT_DIR/.notarize-$$.zip"
  ditto -c -k --keepParent "$APP_SRC" "$ZIP_FOR_NOTARY"
  echo "[notarize] xcrun notarytool submit"
  xcrun notarytool submit "$ZIP_FOR_NOTARY" \
    --apple-id "$APPLE_ID" --team-id "$TEAM_ID" --password "$APP_SPECIFIC_PASSWORD" --wait
  rm -f "$ZIP_FOR_NOTARY"
  echo "[notarize] xcrun stapler staple"
  xcrun stapler staple "$APP_SRC"
else
  echo "[notarize] 跳过（NOTARIZE=${NOTARIZE}；公测阶段不做公证）"
fi

# ---- 5. 打包 zip + sha256 ----------------------------------------------
ZIP_NAME="${APP_NAME}-${VERSION}-${ARCH_LABEL}.zip"
ZIP_PATH="$OUT_DIR/$ZIP_NAME"
echo "[pack] ditto -c -k --keepParent → ${ZIP_NAME}"
rm -f "$ZIP_PATH"
ditto -c -k --keepParent "$APP_SRC" "$ZIP_PATH"

SHA_PATH="$ZIP_PATH.sha256"
( cd "$OUT_DIR" && shasum -a 256 "$ZIP_NAME" > "$ZIP_NAME.sha256" )
echo "[pack] SHA256: $(awk '{print $1}' "$SHA_PATH")"

# ---- 6. 可选 dmg（手动下载用）------------------------------------------
DMG_NAME="${APP_NAME}-${VERSION}-${ARCH_LABEL}.dmg"
DMG_PATH="$OUT_DIR/$DMG_NAME"
if [ "${BUILD_DMG:-1}" = "1" ]; then
  echo "[pack] 生成 dmg：${DMG_NAME}"
  DMG_STAGE="$TMPDIR/mr-dmg-$$"
  mkdir -p "$DMG_STAGE"
  ditto "$APP_SRC" "$DMG_STAGE/${APP_NAME}.app"
  ln -s /Applications "$DMG_STAGE/Applications" 2>/dev/null || true
  hdiutil create -volname "$APP_NAME" -srcfolder "$DMG_STAGE" -ov -format UDZO "$DMG_PATH" >/dev/null
  ( cd "$OUT_DIR" && shasum -a 256 "$DMG_NAME" > "$DMG_NAME.sha256" )
  echo "[pack] dmg SHA256: $(awk '{print $1}' "$DMG_PATH.sha256")"
  rm -rf "$DMG_STAGE"
else
  echo "[pack] 跳过 dmg（BUILD_DMG=0）"
fi

# ---- 7. checksums.txt + VERSION.txt ------------------------------------
# install.sh 靠 VERSION.txt 把 latest 标签解析成真实版本号。
CHECKSUMS="$OUT_DIR/checksums.txt"
: > "$CHECKSUMS"
for f in "$OUT_DIR"/*.sha256; do
  [ -e "$f" ] || continue
  cat "$f" >> "$CHECKSUMS"
done
echo "[pack] 已生成 checksums.txt"

# 版本号文件：供 install.sh 的 latest 解析使用（单一来源仍为 tauri.conf.json）
echo "$VERSION" > "$OUT_DIR/VERSION.txt"
echo "[pack] 已生成 VERSION.txt（${VERSION}）"

# ---- 8. 清理暂存 --------------------------------------------------------
if [ "$ARCH_LABEL" = "universal" ] && [ -d "$OUT_DIR/stage-universal" ]; then
  rm -rf "$OUT_DIR/stage-universal"
fi

echo ""
echo "[done] 产物目录：${OUT_DIR}"
ls -1 "$OUT_DIR"
echo ""
echo "[done] 用户安装命令："
echo "  curl -fsSL <base>/releases/latest/install.sh | bash"
