#!/usr/bin/env python3
"""本地 e2e fixture：上传页面，并把本地 OCR 的 bbox 直接写进 blocks，跳过百度识别。

用途：验证分享页对齐、docx 生成、缓存命中，不消耗 OCR 额度。
前置：服务端已启动，且与 `SHIYE_DATA_DIR` 指向同一个数据目录。

用法：
    SHIYE_DATA_DIR=.tmp/e2e/data \\
    SHIYE_OCR_JSON=.tmp/e2e \\
    python3 tools/e2e/seed.py "三页测试" samples/pages/p01.jpg samples/pages/p02.jpg

每张图旁边（或 `SHIYE_OCR_JSON` 目录下）需要有同名 `.json`：
    {"w": 1387, "h": 2000, "blocks": [{"text": "...", "x1": 1, "y1": 2, "x2": 3, "y2": 4}]}
用 `swift tools/e2e/vision_ocr.swift <图片> > p01.json` 生成。
"""
import json
import os
import sqlite3
import sys
import urllib.request
import uuid

BASE = os.environ.get("SHIYE_BASE", "http://127.0.0.1:8080/shiye/api/v1")
DEV = os.environ.get("SHIYE_DEVICE_ID", "9f1c2b7e-1111-4222-8333-444455556666")
DATA_DIR = os.environ.get("SHIYE_DATA_DIR", "data")
OCR_DIR = os.environ.get("SHIYE_OCR_JSON", ".")
DB = os.path.join(DATA_DIR, "shiye.db")


def api(path, method="GET", body=None):
    req = urllib.request.Request(BASE + path, method=method)
    req.add_header("X-Device-Id", DEV)
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, data=data) as resp:
        raw = resp.read()
    return json.loads(raw) if raw else None


def upload(group_id, path):
    boundary = "----e2e" + uuid.uuid4().hex
    with open(path, "rb") as f:
        content = f.read()
    name = os.path.basename(path)
    body = b"".join([
        f"--{boundary}\r\n".encode(),
        f'Content-Disposition: form-data; name="file"; filename="{name}"\r\n'.encode(),
        b"Content-Type: image/jpeg\r\n\r\n",
        content,
        f"\r\n--{boundary}--\r\n".encode(),
    ])
    req = urllib.request.Request(
        f"{BASE}/groups/{group_id}/pages", data=body, method="POST",
        headers={"X-Device-Id": DEV, "Content-Type": f"multipart/form-data; boundary={boundary}"},
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read())


def ocr_json_for(img):
    stem = os.path.splitext(os.path.basename(img))[0]
    for path in (os.path.join(os.path.dirname(img), stem + ".json"), os.path.join(OCR_DIR, stem + ".json")):
        if os.path.exists(path):
            return path
    raise SystemExit(f"找不到 {stem}.json（放在图片旁边或 SHIYE_OCR_JSON 目录）")


def seed_blocks(page_id, ocr_json):
    data = json.load(open(ocr_json))
    w, h = data["w"], data["h"]
    rows = []
    for b in data["blocks"]:
        text = (b.get("text") or "").strip()
        if not text:
            continue
        x1 = max(0.0, min(w - 1, b["x1"]))
        y1 = max(0.0, min(h - 1, b["y1"]))
        x2 = max(x1 + 2, min(w, b["x2"]))
        y2 = max(y1 + 2, min(h, b["y2"]))
        # 列顺序：page_id, ord, label, text, md, x1, y1, x2, y2
        rows.append((page_id, len(rows), "text", text, text, x1, y1, x2, y2))
    con = sqlite3.connect(DB, timeout=10)
    con.executemany(
        "INSERT OR REPLACE INTO blocks (page_id, ord, label, text, md, x1, y1, x2, y2) "
        "VALUES (?,?,?,?,?,?,?,?,?)",
        rows,
    )
    con.commit()
    con.close()
    return len(rows)


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    title, images = sys.argv[1], sys.argv[2:]
    group = api("/groups", "POST", {"title": title})
    gid = group["id"]
    print(f"group={gid}")

    total = 0
    for img in images:
        info = upload(gid, img)
        pid = info["page_id"]
        total += seed_blocks(pid, ocr_json_for(img))
        print(f"  page={pid} {info['w']}x{info['h']}")

    con = sqlite3.connect(DB, timeout=10)
    con.execute("UPDATE pages SET status='done', error=NULL WHERE group_id=?", (gid,))
    con.execute(
        "UPDATE groups SET state='done', error=NULL, page_count=(SELECT COUNT(*) FROM pages WHERE group_id=?), "
        "done_count=(SELECT COUNT(*) FROM pages WHERE group_id=?) WHERE id=?",
        (gid, gid, gid),
    )
    con.commit()
    con.close()

    share = api(f"/groups/{gid}/share", "POST", {})
    print(f"blocks={total}")
    print(f"url={share['url']}")


if __name__ == "__main__":
    main()
