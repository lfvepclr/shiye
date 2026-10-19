# 测试素材

跑通「上传 → OCR → 生成 docx / 分享页」后，用同一批素材对比版式效果。

```
samples/
├── pages/          输入：11 页课本扫描（长边 2000，JPEG；原图在 mathocr/testset/batch_test1）
├── out/            输出：Word 渲染图、对比页与 docx 成品
│   ├── compare-1p.png   原图 / Word 左右对比
│   ├── compare.html     可直接用浏览器打开的双栏对比页
│   ├── word-1p.png      单页 docx 首页渲染
│   ├── word-3p.png      三页 docx 首页渲染
│   ├── sample-3p.docx   三页 docx 成品
│   └── screens/         客户端与分享页截图（流程 / 编辑 / 叠加 / Word）
└── run.sh          一键跑一页：建组 → 上传 → 渲染 → 下载 docx
```

## 跑一遍

服务端起来后（默认 `http://127.0.0.1:8080/shiye`）：

```bash
SHIYE_DEVICE_ID=$(uuidgen | tr 'A-Z' 'a-z') \
SHIYE_IMG=samples/pages/p01.jpg \
  ./samples/run.sh
```

产物默认落在 `samples/out/run/`，与 `out/` 里已有的基准图并排看即可。

## 截图命名

| 前缀 | 内容 |
| --- | --- |
| `flow-` | 主流程：相机、连拍、组编辑、排序、生成进度、保存 docx |
| `edit-` | 单页编辑：四角矫正、裁剪、擦除 |
| `overlay-` / `viewer-` | 分享页：原文叠加对齐、纯文本、手机单栏 |
| `word-` / `compare-` | Word 渲染与双栏对比 |

## 约定

- `pages/` 只放**输入**素材，按 `pNN.jpg` 编号，顺序即测试顺序。
- `out/` 只放**输出**素材，文件名带得出「哪一页、什么产物」（`word-1p.png`）。
- 新增基准对比图后，在 `out/compare.html` 里加一栏，保持左右对齐同宽，方便肉眼比对。
