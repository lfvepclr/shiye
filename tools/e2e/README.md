# e2e 工具

不消耗百度 OCR 额度的本地验证：用 Apple Vision 出真实 bbox，直接把块写进库，然后量分享页。

```bash
# 0) 依赖：macOS（Vision）、python3、node + playwright-core
npm i playwright-core            # 或在 PLAYWRIGHT_PATH 指向已有安装

# 1) 起服务（假 AK/SK 也行，fixture 不走识别）
SHIYE_CONFIG=.tmp/e2e/config2.toml ./server/target/debug/shiye-server &

# 2) 用本地 OCR 生成 bbox（每页一次）
swift tools/e2e/vision_ocr.swift samples/pages/p01.jpg > .tmp/e2e/p01.json

# 3) 建组、上传、写块、生成分享链接
SHIYE_DATA_DIR=.tmp/e2e/data2 SHIYE_OCR_JSON=.tmp/e2e \
  python3 tools/e2e/seed.py "三页测试" samples/pages/p01.jpg
# → url=http://127.0.0.1:8080/shiye/s/<token>/

# 4) 量 390 / 1080 两档：对齐误差、文字溢出、模糊↔高清、LCP、图片字节
PLAYWRIGHT_PATH=$PWD/.tmp/tools/pw/node_modules/playwright-core \
  node tools/e2e/measure.js "http://127.0.0.1:8080/shiye/s/<token>/"
```

输出是一行 JSON：

| 字段 | 含义 |
| --- | --- |
| `maxErrPct` | 块 DOM 位置与 bbox 的最大偏差（占页宽百分比），阈值 0.4% |
| `clipped` | 文字超出块被裁的块数，应为 0 |
| `thumbCount` / `dispCount` | 当前 `?v=thumb` / `?v=disp` 的图片数 |
| `thumbBytes` | 首屏缩略图传输字节 |
| `lcpMs` | 首图 LCP |
| `hd` / `after` | 点「高清」后与刷新后的状态（含 localStorage） |

注意：`seed.py` 直接改 SQLite，`SHIYE_DATA_DIR` 必须与服务端配置的 `data_dir` 一致；`config.example.toml` 里默认是 `data`。
