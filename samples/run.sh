#!/usr/bin/env bash
# 用 samples/pages 里的一页跑完整链路：建组 → 上传 → 渲染 → 下载 docx。
# 用法：
#   SHIYE_IMG=samples/pages/p01.jpg ./samples/run.sh
# 需要服务端已启动（默认 http://127.0.0.1:8080/shiye）。
set -euo pipefail

BASE="${SHIYE_BASE:-http://127.0.0.1:8080/shiye/api/v1}"
DEV="${SHIYE_DEVICE_ID:-$(uuidgen | tr 'A-Z' 'a-z')}"
IMG="${SHIYE_IMG:-samples/pages/p01.jpg}"
OUT="${SHIYE_OUT:-samples/out/run}"

cd "$(dirname "$0")/.."
[ -f "$IMG" ] || { echo "找不到图片：$IMG" >&2; exit 1; }
mkdir -p "$OUT"

json() { python3 -c "import json,sys; print(json.load(sys.stdin)$1)"; }

GID=$(curl -sf -X POST "$BASE/groups" \
  -H "X-Device-Id: $DEV" -H 'Content-Type: application/json' \
  -d "{\"title\":\"$(basename "$IMG")\"}" | json "['id']")
echo "group=$GID"

curl -sf -X POST "$BASE/groups/$GID/pages" -H "X-Device-Id: $DEV" \
  -F "file=@$IMG" | json "['page_id']" | sed 's/^/page=/'

curl -sf -X POST "$BASE/groups/$GID/render" -H "X-Device-Id: $DEV" >/dev/null

for _ in $(seq 1 120); do
  STATE=$(curl -sf "$BASE/groups/$GID/status" -H "X-Device-Id: $DEV" | json "['state']")
  case "$STATE" in
    recognizing|queued) sleep 2 ;;
    *) break ;;
  esac
done
echo "state=$STATE"
[ "$STATE" = "done" ] || { echo "识别未完成，跳过下载" >&2; exit 1; }

NAME="$OUT/$(basename "${IMG%.*}").docx"
curl -sf -o "$NAME" "$BASE/groups/$GID/docx" -H "X-Device-Id: $DEV"
echo "docx=$NAME"

# 分享页地址，便于在浏览器里核对原文叠加效果
SHARE=$(curl -sf -X POST "$BASE/groups/$GID/share" \
  -H "X-Device-Id: $DEV" -H 'Content-Type: application/json' -d '{}' | json "['url']")
echo "share=$SHARE"
