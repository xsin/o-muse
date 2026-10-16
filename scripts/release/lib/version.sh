#!/usr/bin/env bash
# 版本号单一来源：从 src-tauri/tauri.conf.json 的 version 字段读取。
# 供 build.sh / CI 使用，避免在多处硬编码版本号。

set -euo pipefail

# 定位仓库根（本文件位于 scripts/release/lib/）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
TAURI_CONF="$REPO_ROOT/src-tauri/tauri.conf.json"

# 用 python3 解析 JSON（macOS 自带，无需 jq 依赖）
read_version() {
  python3 - "$TAURI_CONF" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)
print(data["version"])
PY
}

# 读取其他字段，便于命名与打包
read_product_name() {
  python3 - "$TAURI_CONF" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    print(json.load(f).get("productName", "O Muse"))
PY
}

read_identifier() {
  python3 - "$TAURI_CONF" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    print(json.load(f)["identifier"])
PY
}

case "${1:-version}" in
  version)        read_version ;;
  product-name)   read_product_name ;;
  identifier)     read_identifier ;;
  *) echo "用法：version.sh {version|product-name|identifier}" >&2; exit 1 ;;
esac
