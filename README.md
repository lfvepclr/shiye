# 拾页 / Shiye

> 拍完即成篇。

连续拍摄多页纸质文档 → 端侧透视矫正与去阴影 → 百度 PaddleOCR 识别 → 生成 Word，并给出一个能在浏览器里查看、分块复制的预览链接。

## 组成

| 目录 | 内容 |
| --- | --- |
| `app/` | HarmonyOS 客户端（ArkTS + ArkUI V2），支持 HarmonyOS 6 / 7 |
| `server/` | Rust 服务端（axum + tokio + SQLite），`#![forbid(unsafe_code)]` |
| `deploy/` | OpenRC 服务脚本、Caddy 反代模板、部署环境变量示例 |
| `.agent/` | 设计与实施计划（文件名前缀 `YYYYMMDDHHMM-`） |

## 文档

- 设计基线：[`.agent/202610181212-design.md`](.agent/202610181212-design.md)
- 实施计划：[`.agent/202610181212-plan.md`](.agent/202610181212-plan.md)

## 接口前缀

所有对外路由都在 `/shiye` 下，前缀由 `config.toml` 的 `base_path` 控制：

```
/shiye/healthz              探活（不鉴权）
/shiye/api/v1/*             客户端 API（X-Device-Id 鉴权）
/shiye/s/{token}            分享页（token 鉴权，只读）
```

## 本地准备

```bash
# 服务端
cp server/config.toml.example server/config.toml   # 填百度 AK/SK
cargo run --manifest-path server/Cargo.toml

# 部署环境变量（不入库）
cp deploy/local.env.example deploy/local.env

# 客户端默认服务器地址（不入库）
cp app/entry/src/main/resources/rawfile/config.example.json \
   app/entry/src/main/resources/rawfile/config.json
```

## 敏感信息

主机地址、域名、SSH、百度 AK/SK **一律不入库**。仓库内只允许出现 `<HOST>` 占位符。
提交前自检：

```bash
grep -rniE '([0-9]{1,3}\.){3}[0-9]{1,3}|secret_key|BEGIN .*PRIVATE KEY' . --exclude-dir=.git
```

## 许可

私有项目，未授权不得使用。
