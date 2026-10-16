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
      # 必须**不加** --no-bundle：--no-bundle 只产出可执行文件、不产出 .app，
      # 后面的 find 会找不到 bundle 而直接失败。
      (cd "$REPO_ROOT" && CARGO_HOME="${CARGO_HOME:-$REPO_ROOT/.cargo-home}" \
        npm_config_cache="${npm_config_cache:-$REPO_ROOT/.npm-cache}" \
        node_modules/.bin/tauri build --target "$t" --ci)
    done
    # 取两个架构的 .app，以 arm64 为基准把可执行文件 lipo 成 universal
    ARM_APP="$(find "$REPO_ROOT/src-tauri/target/aarch64-apple-darwin/release/bundle/macos" \
      -maxdepth 1 -name "*.app" 2>/dev/null | head -n1)"
    X86_APP="$(find "$REPO_ROOT/src-tauri/target/x86_64-apple-darwin/release/bundle/macos" \
      -maxdepth 1 -name "*.app" 2>/dev/null | head -n1)"
    [ -n "$ARM_APP" ] || { echo "未找到 arm64 .app" >&2; exit 1; }
    APP_BUNDLE="$OUT_DIR/stage-universal"
    rm -rf "$APP_BUNDLE"
    mkdir -p "$APP_BUNDLE"
    ditto "$ARM_APP" "$APP_BUNDLE/${APP_NAME}.app"
    if [ -n "$X86_APP" ]; then
      find "$APP_BUNDLE/${APP_NAME}.app/Contents/MacOS" -type f 2>/dev/null | while read -r arm_bin; do
        # rel 形如 "Contents/MacOS/o-muse"；它相对 .app 根，
        # 故直接拼到 X86_APP 上（此前误写成 "$X86_APP/Contents/${rel#MacOS/}"，
        # 会产生 .../Contents/Contents/MacOS/... 这种不存在的路径，
        # 导致 lipo 从不执行、却仍报"合并完成"——静默产出 arm64-only 的假 universal 包）。
        rel="${arm_bin#"$APP_BUNDLE/${APP_NAME}".app/}"
        x86_bin="$X86_APP/$rel"
        if [ -f "$x86_bin" ]; then
          lipo -create "$arm_bin" "$x86_bin" -output "$arm_bin.tmp" && mv "$arm_bin.tmp" "$arm_bin"
          echo "merged: $(basename "$arm_bin")"
        fi
      done
      echo "[build] lipo 合并完成"
      # 至少合并了一个可执行文件才算 universal；否则宁可失败也不要
      # 产出名不副实的"universal"包（清单会把它标给两个架构）。
      MERGED_MAIN="$(lipo -archs "$APP_BUNDLE/${APP_NAME}.app/Contents/MacOS/${APP_NAME}" 2>/dev/null || true)"
      case "$MERGED_MAIN" in
        *arm64*x86_64*|*x86_64*arm64*) echo "[build] 主可执行文件架构: $MERGED_MAIN" ;;
        *) echo "[build] 错误：主可执行文件未成为 universal（实际：${MERGED_MAIN:-未知}）" >&2; exit 1 ;;
      esac
    else
      echo "[build] 警告：未找到 x86_64 .app，universal 包将只含 arm64" >&2
    fi
  else
    echo "[build] 构建单架构：${ARCH_LABEL}"
    # 必须**不加** --no-bundle：它只产出可执行文件、不产出 .app，
    # 后续找不到 bundle 会直接失败（这正是 CI 报"未找到 .app 产物"的原因）。
    (cd "$REPO_ROOT" && CARGO_HOME="${CARGO_HOME:-$REPO_ROOT/.cargo-home}" \
      npm_config_cache="${npm_config_cache:-$REPO_ROOT/.npm-cache}" \
      node_modules/.bin/tauri build --target "$RUST_TARGETS" --ci)
    # .app 在 target/<triple>/release/bundle/macos/ 下，**不在** release/ 根目录；
    # 之前找 release/ 根导致 CI 报"未找到 .app 产物"（本地只测过 --skip-build 分支所以漏了）。
    APP_BUNDLE="$(find "$REPO_ROOT/src-tauri/target/$RUST_TARGETS/release/bundle/macos" \
      -maxdepth 1 -name "*.app" 2>/dev/null | head -n1)"
    [ -n "$APP_BUNDLE" ] || {
      echo "未找到 .app 产物（已查找 target/$RUST_TARGETS/release/bundle/macos）" >&2
      exit 1
    }
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
  # 3) 可执行文件。顺序很重要：codesign 在签**主可执行文件**时会校验整个
  #    bundle，若此时 sidecar 等附属可执行文件仍是未签名状态，就会报
  #    "code object is not signed at all / In subcomponent"，导致签名失败。
  #    因此先从 Info.plist 读出 CFBundleExecutable，先签其余、**主程序最后签**，
  #    不能依赖字母序（"o-muse" 恰好排在 "python-stem-backend" 之前）。
  if [ -d "$target/Contents/MacOS" ]; then
    MAIN_EXE_NAME="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' \
      "$target/Contents/Info.plist" 2>/dev/null || true)"
    for exe in "$target/Contents/MacOS"/*; do
      [ -f "$exe" ] || continue
      [ "$(basename "$exe")" = "$MAIN_EXE_NAME" ] && continue
      echo "[sign]   executable: $(basename "$exe")"
      codesign "${SIGN_ARGS[@]}" "$exe"
    done
    if [ -n "$MAIN_EXE_NAME" ] && [ -f "$target/Contents/MacOS/$MAIN_EXE_NAME" ]; then
      echo "[sign]   main executable（最后签）: $MAIN_EXE_NAME"
      codesign "${SIGN_ARGS[@]}" "$target/Contents/MacOS/$MAIN_EXE_NAME"
    fi
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
  # dmg 是**可选**产物（主分发是 zip）：失败只告警，不得中断
  # tar.gz / latest.json / checksums 的生成（否则一个可选步骤会毁掉整次发布）。
  if hdiutil create -volname "$APP_NAME" -srcfolder "$DMG_STAGE" -ov -format UDZO "$DMG_PATH" >/dev/null 2>&1; then
    ( cd "$OUT_DIR" && shasum -a 256 "$DMG_NAME" > "$DMG_NAME.sha256" )
    echo "[pack] dmg SHA256: $(awk '{print $1}' "$DMG_PATH.sha256")"
  else
    echo "[pack] 警告：dmg 生成失败（可选产物，已跳过；zip/tar.gz/清单不受影响）" >&2
    rm -f "$DMG_PATH"
  fi
  rm -rf "$DMG_STAGE"
else
  echo "[pack] 跳过 dmg（BUILD_DMG=0）"
fi

# ---- 6.5 更新用归档 .app.tar.gz（自动更新使用，与 zip 同源）-----------
# 客户端无法解压 zip（zip crate 上游被 yank），故额外产出 tar.gz，
# 自动更新器用 flate2 + tar 解压。内容与 zip 完全同源。
TARGZ_NAME="${APP_NAME}-${VERSION}-${ARCH_LABEL}.app.tar.gz"
TARGZ_PATH="$OUT_DIR/$TARGZ_NAME"
echo "[pack] tar.gz → ${TARGZ_NAME}"
rm -f "$TARGZ_PATH"
# --mac-metadata（bsdtar）：保留扩展属性等 macOS 元数据。
# 侧车 python-stem-backend 是**脚本**，其 codesign 签名存放在 xattr 中，
# 不保留就会在解压后丢失、导致整个 bundle 签名校验失败。
# 注意：客户端用的 Rust tar crate 只认 SCHILY.xattr.* 前缀，与 bsdtar 的
# LIBARCHIVE.xattr.* 不一致，故客户端仍会在解压后**重新签名**兜底
# （见 core/updater 与 tdd-auto-update.md）。此处保留仍有益无害。
if tar --help 2>&1 | grep -q -- "--mac-metadata" || tar --mac-metadata -cf /dev/null --files-from /dev/null 2>/dev/null; then
  COPYFILE_DISABLE=1 tar --mac-metadata -czf "$TARGZ_PATH" -C "$(dirname "$APP_SRC")" "$(basename "$APP_SRC")"
else
  echo "[pack] 警告：tar 不支持 --mac-metadata（非 bsdtar），xattr 不会被保留；客户端会重新签名兜底" >&2
  COPYFILE_DISABLE=1 tar -czf "$TARGZ_PATH" -C "$(dirname "$APP_SRC")" "$(basename "$APP_SRC")"
fi
( cd "$OUT_DIR" && shasum -a 256 "$TARGZ_NAME" > "$TARGZ_NAME.sha256" )
TARGZ_SHA="$(awk '{print $1}' "$TARGZ_PATH.sha256")"
echo "[pack] tar.gz SHA256: ${TARGZ_SHA}"

# ---- 6.6 latest.json（自动更新清单，支持多架构合并）-------------------
# 用 python3 生成，因为它需要**合并**已有清单：CI 会分别构建 arm64 与 x86_64
# （两条单架构路径都已实测），第二次运行必须把新平台追加进去而不是覆盖。
LATEST_JSON="$OUT_DIR/latest.json"
RELEASE_BASE="${OMUSE_RELEASE_BASE:-https://xsin.github.io/o-muse/releases}"
PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# 平台键必须与客户端一致：Rust 端按 std::env::consts::ARCH 映射，
# aarch64 → darwin-aarch64、x86_64 → darwin-x86_64。
# 注意产物命名用的是 "arm64"，与平台键的 "aarch64" **不同**，切勿混用。
case "$ARCH_LABEL" in
  arm64)  PLATFORM_KEYS="darwin-aarch64" ;;
  x86_64) PLATFORM_KEYS="darwin-x86_64" ;;
  universal) PLATFORM_KEYS="darwin-aarch64 darwin-x86_64" ;;
  *) echo "未知架构标签：$ARCH_LABEL" >&2; exit 1 ;;
esac

python3 - "$LATEST_JSON" "$VERSION" "$PUB_DATE" "$RELEASE_BASE" "$TARGZ_NAME" "$TARGZ_SHA" "$PLATFORM_KEYS" <<'PY'
import json, os, sys
from urllib.parse import quote

path, version, pub_date, base, tgz_name, tgz_sha, keys = sys.argv[1:8]
keys = keys.split()

existing = {}
if os.path.exists(path):
    try:
        with open(path, encoding="utf-8") as f:
            existing = json.load(f).get("platforms") or {}
    except Exception:
        existing = {}

url = f"{base}/{version}/{quote(tgz_name)}"
for k in keys:
    existing[k] = {"url": url, "sha256": tgz_sha}

doc = {
    "version": version,
    "notes": os.environ.get("OMUSE_UPDATE_NOTES", f"O Muse {version} 更新"),
    "pub_date": pub_date,
    "platforms": existing,
}
with open(path, "w", encoding="utf-8") as f:
    json.dump(doc, f, ensure_ascii=False, indent=2)
    f.write("\n")
print(f"[pack] 已生成 latest.json（本次平台：{keys}，累计：{sorted(existing)}）")
PY

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
