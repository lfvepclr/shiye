//! OCR 结果块模型、百度结果适配与 HTML 清洗。
//!
//! 统一块模型：一份 `Vec<Block>` 同时服务 Web Viewer、富文本复制与 Word 导出。

use serde::{Deserialize, Serialize};

/// 一个识别块。坐标为原图像素系的 `[x1,y1,x2,y2]`。
/// `text` 为纯文本；`md` 为块级 markdown（docx / 富文本 / Viewer 文本视图的输入）。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Block {
    pub ord: i64,
    pub label: String,
    pub text: String,
    pub md: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub poly: Option<String>,
}

/// 一页的适配结果。
#[derive(Debug, Clone)]
pub struct AdaptedPage {
    pub width: u32,
    pub height: u32,
    pub blocks: Vec<Block>,
    /// 阅读顺序拼接的 markdown（docx 与富文本的输入）。
    pub markdown: String,
}

const LABEL_MAP: &[(&str, &str)] = &[
    ("title", "paragraph_title"),
    ("display_formula", "formula"),
    ("content", "contents"),
];

const IMAGE_TYPES: &[&str] = &["image", "header_image", "footer_image", "chart", "seal"];

fn map_label(baidu_type: &str) -> String {
    for (from, to) in LABEL_MAP {
        if *from == baidu_type {
            return (*to).to_string();
        }
    }
    baidu_type.to_string()
}

fn is_image_type(t: &str) -> bool {
    IMAGE_TYPES.contains(&t)
}

/// 标题层级：`doc_title`→1；`sub_type` 尾位数字；`1.`/`1、` 编号开头降为 3。
fn title_level(sub_type: &str, text: &str) -> usize {
    let digits: String = sub_type.chars().filter(|c| c.is_ascii_digit()).collect();
    let mut level: usize = digits.parse().unwrap_or(2).clamp(1, 6);
    if level <= 2 {
        let numbered = text
            .trim_start()
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
            && {
                let t = text.trim_start();
                let after = t.trim_start_matches(|c: char| c.is_ascii_digit());
                after.starts_with('.') || after.starts_with('、')
            };
        if numbered {
            level = 3;
        }
    }
    level
}

fn escape_html(s: &str) -> String {
    // 不重复转义已有实体（百度表格 markdown 可能带 &nbsp; 等）
    let mut out = String::with_capacity(s.len() + 32);
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let keep = rest[1..]
            .find(';')
            .filter(|end| *end <= 10)
            .map(|end| {
                let body = &rest[1..1 + end];
                !body.is_empty()
                    && body
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '#')
            })
            .unwrap_or(false);
        if keep {
            let end = rest[1..].find(';').unwrap() + 2;
            out.push_str(&rest[..end]);
            rest = &rest[end..];
        } else {
            out.push_str("&amp;");
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out.replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// HTML 白名单清洗：只保留表格与少数行内标签，其余全部转义。
/// `img` 只允许本地相对引用（fig 名），杜绝注入远程地址与事件属性。
pub fn sanitize_html(raw: &str) -> String {
    let allowed = [
        "table", "thead", "tbody", "tr", "td", "th", "br", "b", "i", "em", "strong", "sub", "sup",
        "div", "p",
    ];
    let mut out = String::with_capacity(raw.len() + 32);
    let mut rest = raw;
    while let Some(open) = rest.find('<') {
        out.push_str(&escape_html(&rest[..open]));
        rest = &rest[open..];
        let close = match rest.find('>') {
            Some(c) => c,
            None => {
                out.push_str(&escape_html(rest));
                return out;
            }
        };
        let tag_src = &rest[..=close];
        rest = &rest[close + 1..];

        let inner = tag_src
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim();
        let (name_part, _attrs_src) = match inner.find(char::is_whitespace) {
            Some(sp) => (&inner[..sp], &inner[sp..]),
            None => (inner, ""),
        };
        let closing = name_part.starts_with('/');
        let name = name_part.trim_start_matches('/').to_ascii_lowercase();

        if name == "img" && !closing {
            // 只保留本地 fig 引用
            let src = extract_attr(tag_src, "src");
            let safe_name = sanitize_fig_name(src.as_deref().unwrap_or(""));
            if let Some(n) = safe_name {
                out.push_str(&format!("<img src=\"{n}\" alt=\"\" loading=\"lazy\">"));
            }
            continue;
        }
        if allowed.contains(&name.as_str()) {
            // 白名单标签不带任何属性输出
            out.push_str(&format!(
                "{}{name}{}",
                if closing { "</" } else { "<" },
                ">"
            ));
            continue;
        }
        // 其他标签整段丢弃
    }
    out.push_str(&escape_html(rest));
    out
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{attr}=");
    let pos = lower.find(&needle)?;
    let after = &tag[pos + needle.len()..];
    let quote = after.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = after[1..].find(quote)? + 1;
        Some(after[1..end].to_string())
    } else {
        let end = after
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(after.len());
        Some(after[..end].to_string())
    }
}

/// fig 文件名只允许 `fig_<hex>.<png|jpg>` 形式。
pub fn sanitize_fig_name(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    if !stem.starts_with("fig_") {
        return None;
    }
    let hex = &stem[4..];
    if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let ext = ext.to_ascii_lowercase();
    if ext == "png" || ext == "jpg" || ext == "jpeg" {
        Some(format!("fig_{hex}.{ext}"))
    } else {
        None
    }
}

/// 纯文本（供剪贴板纯文本路径与 docx 段落）。
/// 行内公式：`$...$` / `$$...$$` 交给 LaTeX 转换，其余原样保留。
pub fn inline_math(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let delim = if tail.starts_with("$$") { "$$" } else { "$" };
        let body_start = delim.len();
        if let Some(end) = tail[body_start..].find(delim) {
            out.push_str(&crate::latex::to_unicode(&tail[body_start..body_start + end]));
            rest = &tail[body_start + end + delim.len()..];
        } else {
            out.push_str(tail);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

/// HTML 实体解码（OCR 的 md 里常见 `&amp;`、`&nbsp;`）。
pub fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// 去掉 markdown 行首标记（`#`、`>`）与强调符，正文里不该出现这些。
fn strip_md_marks(text: &str) -> String {
    text.lines()
        .map(|line| {
            let t = line.trim();
            let t = t.trim_start_matches('#').trim_start();
            let t = t.trim_start_matches('>').trim_start();
            t.replace("**", "").replace("__", "")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 块 → 纯文本：去标签、解实体、行内公式转 Unicode、去 markdown 标记。Word 与网页共用。
pub fn plain_text(md: &str) -> String {
    let flat = md
        .replace("<br/>", " ")
        .replace("<br />", " ")
        .replace("<br>", " ");
    strip_md_marks(&inline_math(&decode_entities(&strip_html(&flat))))
}

pub fn strip_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('>').map(|c| open + c);
        match close {
            Some(c) => rest = &rest[c + 1..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// 百度 JSON → 统一块模型
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BaiduPage {
    #[serde(default)]
    meta: BaiduMeta,
    #[serde(default)]
    layouts: Vec<BaiduLayout>,
    #[serde(default)]
    tables: Vec<BaiduTable>,
    #[serde(default)]
    images: Vec<BaiduFigure>,
}

#[derive(Debug, Default, Deserialize)]
struct BaiduMeta {
    #[serde(default)]
    page_width: u32,
    #[serde(default)]
    page_height: u32,
}

#[derive(Debug, Default, Deserialize)]
struct BaiduLayout {
    #[serde(default)]
    layout_id: serde_json::Value,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    position: Vec<f64>,
    #[serde(default)]
    polygon: serde_json::Value,
    #[serde(default)]
    sub_type: String,
}

#[derive(Debug, Default, Deserialize)]
struct BaiduTable {
    #[serde(default)]
    layout_id: serde_json::Value,
    #[serde(default)]
    markdown: String,
}

#[derive(Debug, Default, Deserialize)]
struct BaiduFigure {
    #[serde(default)]
    layout_id: serde_json::Value,
    #[serde(default)]
    image_description: String,
}

fn layout_id_matches(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    a == b && !a.is_null()
}

fn bbox(position: &[f64]) -> Option<(f64, f64, f64, f64)> {
    if position.len() != 4 {
        return None;
    }
    let (x, y, w, h) = (position[0], position[1], position[2], position[3]);
    Some((x, y, x + w, y + h))
}

/// 适配一页百度结果。
/// `figs` 为已下载的插图（文件名 → 字节），适配层只引用文件名。
pub fn adapt_page(page_json: &serde_json::Value, fig_names: &[String]) -> anyhow::Result<AdaptedPage> {
    let page: BaiduPage =
        serde_json::from_value(page_json.clone()).map_err(|e| anyhow::anyhow!("解析百度页结果失败: {e}"))?;
    let width = page.meta.page_width.max(1);
    let height = page.meta.page_height.max(1);

    let mut fig_iter = fig_names.iter();
    let mut blocks = Vec::new();
    let mut md_parts: Vec<String> = Vec::new();

    for layout in &page.layouts {
        let kind = if layout.kind.is_empty() { "text" } else { &layout.kind };
        let label = map_label(kind);
        let Some((x1, y1, x2, y2)) = bbox(&layout.position) else {
            continue;
        };
        let ord = blocks.len() as i64 + 1;
        let text_raw = layout.text.trim();

        let (text, md) = if kind == "table" {
            let table = page
                .tables
                .iter()
                .find(|t| layout_id_matches(&t.layout_id, &layout.layout_id));
            let content = table.map(|t| t.markdown.as_str()).unwrap_or(text_raw).to_string();
            (content.clone(), content)
        } else if is_image_type(kind) {
            let fig = page
                .images
                .iter()
                .find(|f| layout_id_matches(&f.layout_id, &layout.layout_id));
            let name = fig.and_then(|_| fig_iter.next()).cloned().unwrap_or_default();
            if name.is_empty() {
                (String::new(), String::new())
            } else {
                let pct = ((x2 - x1) / width as f64 * 100.0)
                    .round()
                    .clamp(5.0, 100.0) as u32;
                let desc = fig
                    .map(|f| f.image_description.trim())
                    .filter(|d| !d.is_empty())
                    .map(|d| format!("\n\n{}", escape_html(d)))
                    .unwrap_or_default();
                let content = format!("<img src=\"{name}\">");
                let md = format!("<div><img src=\"{name}\" width=\"{pct}%\"></div>{desc}");
                (content, md)
            }
        } else if kind == "display_formula" {
            let t = text_raw.trim();
            let md = if t.is_empty() { String::new() } else { format!("$${t}$$") };
            (t.to_string(), md)
        } else if kind == "inline_formula" {
            let t = text_raw.trim();
            let md = if t.is_empty() || t.contains('$') {
                t.to_string()
            } else {
                format!("${t}$")
            };
            (t.to_string(), md)
        } else if label == "doc_title" || label == "paragraph_title" {
            let level = title_level(&layout.sub_type, text_raw);
            let md = if text_raw.is_empty() {
                String::new()
            } else {
                format!("{} {}", "#".repeat(level), escape_html(text_raw))
            };
            (text_raw.to_string(), md)
        } else if label == "figure_title" || label == "table_title" {
            let md = if text_raw.is_empty() {
                String::new()
            } else {
                format!("<div>{}</div>", escape_html(text_raw))
            };
            (text_raw.to_string(), md)
        } else {
            (text_raw.to_string(), escape_html(text_raw))
        };

        let poly = layout
            .polygon
            .as_array()
            .map(|pts| {
                pts.iter()
                    .filter_map(|p| p.as_f64())
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .filter(|s| !s.is_empty());

        if !md.trim().is_empty() {
            md_parts.push(md.clone());
        }
        blocks.push(Block {
            ord,
            label,
            text,
            md,
            x1,
            y1,
            x2,
            y2,
            poly,
        });
    }

    Ok(AdaptedPage {
        width,
        height,
        blocks,
        markdown: md_parts.join("\n\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_table_drops_script() {
        let out = sanitize_html("<table><tr><td>a&amp;b</td></tr></table><script>alert(1)</script>");
        assert!(out.contains("<table>"));
        assert!(out.contains("a&amp;b")); // 已有实体不重复转义
        assert!(!out.contains("script"));
    }

    #[test]
    fn plain_text_converts_inline_math() {
        assert_eq!(
            plain_text("在 $ \\triangle ABC $ 中，AD 是 $ \\triangle ABC $ 的高"),
            "在 △ ABC 中，AD 是 △ ABC 的高"
        );
        assert_eq!(plain_text("<div>图1-1-5</div>"), "图1-1-5");
    }

    #[test]
    fn sanitize_img_only_local_fig() {
        let out = sanitize_html("<img src=\"fig_ab12.png\" onerror=\"x\"><img src=\"http://evil/x.png\">");
        assert!(out.contains("src=\"fig_ab12.png\""));
        assert!(!out.contains("onerror"));
        assert!(!out.contains("http://evil"));
    }

    #[test]
    fn strip_html_works() {
        assert_eq!(strip_html("<p>你好<i>世界</i></p>"), "你好世界");
    }

    #[test]
    fn title_levels() {
        assert_eq!(title_level("", "文档标题"), 2);
        assert_eq!(title_level("sub_type2", "第一章"), 2);
        assert_eq!(title_level("", "1. 引言"), 3);
        assert_eq!(title_level("", "3、方法"), 3);
    }

    #[test]
    fn adapts_a_minimal_page() {
        let page = serde_json::json!({
            "meta": {"page_width": 1000, "page_height": 1400},
            "layouts": [
                {"layout_id": 1, "type": "title", "text": "文档标题", "position": [10, 10, 300, 40]},
                {"layout_id": 2, "type": "content", "text": "正文<b>加粗</b>", "position": [10, 60, 900, 30]}
            ],
            "tables": [], "images": []
        });
        let adapted = adapt_page(&page, &[]).unwrap();
        assert_eq!(adapted.blocks.len(), 2);
        assert_eq!(adapted.blocks[0].label, "paragraph_title");
        assert!(adapted.markdown.starts_with('#'));
        assert!(adapted.markdown.contains("&lt;b&gt;"));
    }
}
