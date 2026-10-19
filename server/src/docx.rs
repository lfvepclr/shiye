//! Word 生成：把统一块模型转成 docx（docx-rs）。
//!
//! 输入是每页的块序列（`md` 为块级 markdown / 受限 HTML），输出是完整 docx 字节。
//! 纯同步、无 IO 依赖：插图字节由调用方通过 `load_fig` 提供。

use anyhow::anyhow;
use docx_rs::{
    AlignmentType, Docx, LineSpacing, PageMargin, Paragraph, Pic, Run, RunFonts,
    SpecialIndentType, Table, TableAlignmentType, TableCell, TableLayoutType, TableRow,
    VAlignType, WidthType,
};

use crate::blocks::{plain_text, Block};
use crate::latex;

const FONT_BODY: &str = "等线";
const FONT_HEAD: &str = "黑体";
const EMU_PER_PX: f64 = 9525.0;
const EMU_PER_TWIP: f64 = 635.0;

/// A4 版心：21cm × 29.7cm，上下 2.54cm、左右 2cm 边距（单位 twip）。
const PAGE_W: f64 = 11_906.0;
const PAGE_H: f64 = 16_838.0;
const MARGIN_TB: i32 = 1_440;
const MARGIN_LR: i32 = 1_134;
const TEXT_W: f64 = PAGE_W - 2.0 * (MARGIN_LR as f64);
const TEXT_W_EMU: f64 = TEXT_W * EMU_PER_TWIP;

/// 一页的输入。`w`/`h` 是原图像素尺寸，用来按版面比例还原插图大小。
pub struct PageDoc {
    pub page_id: String,
    pub w: f64,
    pub h: f64,
    pub blocks: Vec<Block>,
}

/// 生成 docx 字节。`load_fig` 依据 `(page_id, fig 文件名)` 返回图片字节。
pub fn build(
    title: &str,
    pages: &[PageDoc],
    load_fig: &dyn Fn(&str, &str) -> Option<Vec<u8>>,
) -> anyhow::Result<Vec<u8>> {
    let mut doc = Docx::new()
        .page_size(PAGE_W as u32, PAGE_H as u32)
        .page_margin(
            PageMargin::new()
                .top(MARGIN_TB)
                .bottom(MARGIN_TB)
                .left(MARGIN_LR)
                .right(MARGIN_LR)
                .header(851)
                .footer(992),
        );

    doc = doc.add_paragraph(heading(title, 1));

    for (idx, page) in pages.iter().enumerate() {
        if idx > 0 {
            doc = doc.add_paragraph(Paragraph::new().page_break_before(true));
        }
        let mut emitted = false;
        match page_flow(page) {
            Some(items) => {
                for item in items {
                    doc = match item {
                        FlowItem::Full(b) => emit_block(doc, b, page, load_fig),
                        FlowItem::TwoCol(l, r) => doc.add_table(columns_table(&l, &r, page, load_fig)),
                    };
                    emitted = true;
                }
            }
            None => {
                for block in &page.blocks {
                    doc = emit_block(doc, block, page, load_fig);
                    emitted = true;
                }
            }
        }
        if !emitted {
            doc = doc.add_paragraph(paragraph("（本页无可识别内容）"));
        }
    }

    let mut buf = std::io::Cursor::new(Vec::new());
    doc.pack(&mut buf)
        .map_err(|e| anyhow!("打包 docx 失败: {e}"))?;
    Ok(buf.into_inner())
}

fn emit_block(
    doc: Docx,
    block: &Block,
    page: &PageDoc,
    load_fig: &dyn Fn(&str, &str) -> Option<Vec<u8>>,
) -> Docx {
    if block.label == "table" {
        if let Some(table) = table_block(block.md.trim(), page, load_fig) {
            return doc.add_table(table);
        }
    }
    let mut doc = doc;
    for p in block_paragraphs(block, page, load_fig) {
        doc = doc.add_paragraph(p);
    }
    doc
}

/// 一个块展开成若干段落；表格块退化为文字（表格塞不进段落流）。
fn block_paragraphs(
    block: &Block,
    page: &PageDoc,
    load_fig: &dyn Fn(&str, &str) -> Option<Vec<u8>>,
) -> Vec<Paragraph> {
    let md = block.md.trim();
    let mut out = Vec::new();
    match block.label.as_str() {
        "image" | "chart" | "seal" | "header_image" | "footer_image" => {
            let mut found = false;
            for name in img_srcs(md) {
                if let Some(bytes) = load_fig(&page.page_id, &name) {
                    if let Some(pic) = picture(&bytes, block, page) {
                        out.push(
                            Paragraph::new()
                                .align(AlignmentType::Center)
                                .add_run(Run::new().add_image(pic)),
                        );
                        found = true;
                    }
                }
            }
            if !found {
                let text = plain_text(md);
                if !text.trim().is_empty() {
                    out.push(paragraph(text.trim()));
                }
            }
        }
        "formula" => out.push(
            Paragraph::new()
                .align(AlignmentType::Center)
                .line_spacing(LineSpacing::new().line(320).after(60))
                .add_run(
                    Run::new()
                        .add_text(&latex::to_unicode(&block.text))
                        .size(22)
                        .fonts(RunFonts::new().ascii("Cambria Math").east_asia(FONT_BODY)),
                ),
        ),
        "doc_title" | "paragraph_title" => {
            let level = md.chars().take_while(|c| *c == '#').count().clamp(1, 6);
            let text = if md.starts_with('#') {
                md.trim_start_matches('#').trim()
            } else {
                block.text.trim()
            };
            if !text.is_empty() {
                out.push(heading(text, level));
            }
        }
        "table" => {
            let raw = if md.is_empty() { block.text.as_str() } else { md };
            let text = plain_text(raw);
            if !text.trim().is_empty() {
                out.push(paragraph(text.trim()));
            }
        }
        _ => {
            let body = md
                .replace("<br/>", "\n")
                .replace("<br />", "\n")
                .replace("<br>", "\n");
            let plain = plain_text(&body);
            for line in plain.split('\n') {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if is_caption(line) {
                    out.push(caption(line));
                } else {
                    out.push(paragraph(line));
                }
            }
        }
    }
    out
}

/// 双栏页的排布单元：整宽块，或一段左右两栏。
enum FlowItem<'a> {
    Full(&'a Block),
    TwoCol(Vec<&'a Block>, Vec<&'a Block>),
}

/// 判断这一页是不是双栏；是则返回按「整宽块 / 两栏段」切好的流。
fn page_flow(page: &PageDoc) -> Option<Vec<FlowItem<'_>>> {
    if page.w <= 0.0 || page.blocks.len() < 8 {
        return None;
    }
    let mid = page.w / 2.0;
    let mut items: Vec<FlowItem> = Vec::new();
    let mut band: Vec<&Block> = Vec::new();
    let mut left_n = 0usize;
    let mut right_n = 0usize;

    for block in &page.blocks {
        if block.x2 - block.x1 > page.w * 0.55 {
            if !band.is_empty() {
                items.extend(band_items(&band, mid));
                band.clear();
            }
            items.push(FlowItem::Full(block));
        } else {
            if (block.x1 + block.x2) / 2.0 < mid {
                left_n += 1;
            } else {
                right_n += 1;
            }
            band.push(block);
        }
    }
    if !band.is_empty() {
        items.extend(band_items(&band, mid));
    }
    if left_n < 3 || right_n < 3 {
        return None;
    }
    Some(items)
}

fn band_items<'a>(band: &[&'a Block], mid: f64) -> Vec<FlowItem<'a>> {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for b in band {
        if (b.x1 + b.x2) / 2.0 < mid {
            left.push(*b);
        } else {
            right.push(*b);
        }
    }
    // 只有一侧时退回顺序排列，不要因为个别段落放弃整页双栏
    if left.is_empty() || right.is_empty() {
        return band.iter().map(|b| FlowItem::Full(b)).collect();
    }
    vec![FlowItem::TwoCol(left, right)]
}

/// 无边框两列表格模拟双栏：比 Word 的分节分栏更可控，也不会影响其他页。
fn columns_table(
    left: &[&Block],
    right: &[&Block],
    page: &PageDoc,
    load_fig: &dyn Fn(&str, &str) -> Option<Vec<u8>>,
) -> Table {
    let half = (TEXT_W / 2.0) as usize;
    let cell = |blocks: &[&Block]| -> TableCell {
        let mut c = TableCell::new().width(half, WidthType::Dxa);
        for b in blocks {
            for p in block_paragraphs(b, page, load_fig) {
                c = c.add_paragraph(p);
            }
        }
        if blocks.is_empty() {
            c = c.add_paragraph(Paragraph::new());
        }
        c
    };
    Table::without_borders(vec![TableRow::new(vec![cell(left), cell(right)])])
        .set_grid(vec![half, half])
        .width(TEXT_W as usize, WidthType::Dxa)
        .layout(TableLayoutType::Fixed)
}

/// 教材里的图注：短、以「图」开头、不含句中说明。
fn is_caption(text: &str) -> bool {
    let t = text.trim();
    if !(t.starts_with("图 ") || t.starts_with("图1") || t.starts_with("图 1")) {
        return false;
    }
    let chars = t.chars().count();
    chars <= 24 && !t.contains("所示") && !t.contains("，")
}

fn body_fonts() -> RunFonts {
    RunFonts::new().ascii(FONT_BODY).east_asia(FONT_BODY)
}

fn head_fonts() -> RunFonts {
    RunFonts::new().ascii(FONT_HEAD).east_asia(FONT_HEAD)
}

/// 正文段落：等线 11pt，两端对齐，首行缩进 2 字符，行距 1.5。
fn paragraph(text: &str) -> Paragraph {
    Paragraph::new()
        .align(AlignmentType::Both)
        // docx-rs 的 first_line_chars 只在已有 indent 时生效；先用 twips 给 2 字符兜底
        .indent(None, Some(SpecialIndentType::FirstLine(440)), None, None)
        .first_line_chars(200)
        .line_spacing(LineSpacing::new().line(360).after(40))
        .add_run(
            Run::new()
                .add_text(text)
                .size(22)
                .fonts(body_fonts()),
        )
}

/// 图注：居中、小一号、紧贴插图。
fn caption(text: &str) -> Paragraph {
    Paragraph::new()
        .align(AlignmentType::Center)
        .line_spacing(LineSpacing::new().line(260).before(40).after(120))
        .add_run(Run::new().add_text(text).size(18).fonts(body_fonts()))
}

/// 标题段落：层级越深字号越小，一级居中。
fn heading(text: &str, level: usize) -> Paragraph {
    let (size, align) = match level {
        1 => (32, AlignmentType::Center),
        2 => (28, AlignmentType::Left),
        3 => (24, AlignmentType::Left),
        _ => (22, AlignmentType::Left),
    };
    Paragraph::new()
        .align(align)
        .line_spacing(LineSpacing::new().before(240).after(120))
        .add_run(
            Run::new()
                .add_text(text)
                .size(size)
                .bold()
                .fonts(head_fonts()),
        )
}

/// 插图宽度按 bbox 占页宽的比例还原到版心，保证和原版式一致。
fn picture(bytes: &[u8], block: &Block, page: &PageDoc) -> Option<Pic> {
    let img = image::load_from_memory(bytes).ok()?;
    let (pw, ph) = (img.width() as f64, img.height() as f64);
    if pw <= 0.0 || ph <= 0.0 {
        return None;
    }
    let bbox_w = block.x2 - block.x1;
    let width = if page.w > 0.0 && bbox_w > 0.0 {
        (bbox_w / page.w * TEXT_W_EMU).clamp(EMU_PER_PX * 6.0, TEXT_W_EMU)
    } else {
        (pw * EMU_PER_PX).min(TEXT_W_EMU)
    };
    // 高度不超过版心，避免一张大图被挤出页面
    let max_h = (PAGE_H - 2.0 * (MARGIN_TB as f64)) * EMU_PER_TWIP;
    let mut height = width * ph / pw;
    let mut width = width;
    if height > max_h {
        height = max_h;
        width = height * pw / ph;
    }
    Some(Pic::new(bytes).size(width as u32, height as u32))
}

/// 收集 markdown 里的本地 fig 文件名。
fn img_srcs(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = md;
    while let Some(pos) = rest.find("<img") {
        let tail = &rest[pos..];
        let Some(end) = tail.find('>') else { break };
        let tag = &tail[..=end];
        rest = &tail[end + 1..];
        if let Some(src) = attr_value(tag, "src") {
            if let Some(safe) = crate::blocks::sanitize_fig_name(&src) {
                out.push(safe);
            }
        }
    }
    out
}

fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let pos = lower.find(&format!("{attr}="))?;
    let after = &tag[pos + attr.len() + 1..];
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

/// 表格：优先 HTML（`<table>`），其次 PaddleOCR 的 markdown 管道表格。
fn table_block(
    md: &str,
    page: &PageDoc,
    load_fig: &dyn Fn(&str, &str) -> Option<Vec<u8>>,
) -> Option<Table> {
    let mut rows = parse_table_rows(md);
    if rows.is_empty() {
        rows = parse_md_table_rows(md);
    }
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return None;
    }
    let cell_w = TEXT_W_EMU / cols as f64 * 0.92;
    let table_rows: Vec<TableRow> = rows
        .iter()
        .enumerate()
        .map(|(row_idx, cells)| {
            let out_cells: Vec<TableCell> = (0..cols)
                .map(|col| {
                    let raw = cells.get(col).cloned().unwrap_or_default();
                    cell(&raw, page, load_fig, cell_w, row_idx == 0)
                })
                .collect();
            TableRow::new(out_cells)
        })
        .collect();

    Some(
        Table::new(table_rows)
            .style("Table Grid")
            .align(TableAlignmentType::Center),
    )
}

/// 单元格：能取到本地插图就放图，否则放文字。
fn cell(
    raw: &str,
    page: &PageDoc,
    load_fig: &dyn Fn(&str, &str) -> Option<Vec<u8>>,
    width_emu: f64,
    bold: bool,
) -> TableCell {
    if let Some(name) = img_srcs(raw).first() {
        if let Some(bytes) = load_fig(&page.page_id, name) {
            if let Some(pic) = cell_picture(&bytes, width_emu) {
                return TableCell::new()
                    .vertical_align(VAlignType::Center)
                    .add_paragraph(
                        Paragraph::new()
                            .align(AlignmentType::Center)
                            .add_run(Run::new().add_image(pic)),
                    );
            }
        }
    }
    let text = plain_text(raw);
    let mut run = Run::new()
        .add_text(text.trim())
        .size(20)
        .fonts(body_fonts());
    if bold {
        run = run.bold();
    }
    TableCell::new()
        .vertical_align(VAlignType::Center)
        .add_paragraph(Paragraph::new().align(AlignmentType::Center).add_run(run))
}

fn cell_picture(bytes: &[u8], max_w: f64) -> Option<Pic> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width() as f64, img.height() as f64);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    // 单元格里的插图限制在 1.5 英寸高，否则一行表格能撑满整页
    let max_h = 1.5 * 914_400.0;
    let mut width = (w * EMU_PER_PX).min(max_w);
    let mut height = width * h / w;
    if height > max_h {
        height = max_h;
        width = height * w / h;
    }
    Some(Pic::new(bytes).size(width as u32, height as u32))
}

/// markdown 管道表格：`| a | b |` + `| --- | --- |`。
fn parse_md_table_rows(md: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for line in md.lines() {
        let t = line.trim();
        if !t.starts_with('|') {
            continue;
        }
        if t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
            continue;
        }
        let cells: Vec<String> = t
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect();
        if cells.iter().all(|c| c.is_empty()) {
            continue;
        }
        rows.push(cells);
    }
    rows
}

fn parse_table_rows(html: &str) -> Vec<Vec<String>> {
    let lower = html.to_ascii_lowercase();
    let mut rows = Vec::new();
    let mut pos = 0usize;
    while let Some(rel) = lower[pos..].find("<tr") {
        let start = pos + rel;
        let Some(gt) = lower[start..].find('>') else { break };
        let content_start = start + gt + 1;
        let content_end = lower[content_start..]
            .find("</tr>")
            .map(|e| content_start + e)
            .unwrap_or(html.len());
        if content_end > content_start {
            rows.push(parse_cells(&html[content_start..content_end]));
        }
        pos = content_end;
    }
    rows
}

fn parse_cells(row_html: &str) -> Vec<String> {
    let lower = row_html.to_ascii_lowercase();
    let mut cells = Vec::new();
    let mut pos = 0usize;
    loop {
        let td = lower[pos..].find("<td").map(|i| pos + i);
        let th = lower[pos..].find("<th").map(|i| pos + i);
        let start = match (td, th) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };
        let Some(gt) = lower[start..].find('>') else { break };
        let content_start = start + gt + 1;
        let end = [lower[content_start..].find("</td>"), lower[content_start..].find("</th>")]
            .into_iter()
            .flatten()
            .min()
            .map(|e| content_start + e)
            .unwrap_or(row_html.len());
        let raw = &row_html[content_start..end];
        let text = plain_text(&raw.replace("<br>", " ").replace("<br/>", " "));
        cells.push(text.trim().to_string());
        pos = end;
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(label: &str, text: &str, md: &str) -> Block {
        Block {
            ord: 1,
            label: label.to_string(),
            text: text.to_string(),
            md: md.to_string(),
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
            poly: None,
        }
    }

    #[test]
    fn builds_a_minimal_docx() {
        let pages = vec![PageDoc {
            page_id: "p1".to_string(),
            w: 1200.0,
            h: 1600.0,
            blocks: vec![
                block("paragraph_title", "第一章", "# 第一章"),
                block("contents", "正文内容", "正文内容"),
            ],
        }];
        let bytes = build("测试文档", &pages, &|_, _| None).unwrap();
        // docx 是 zip：前两字节固定为 PK。
        assert!(bytes.starts_with(b"PK"));
        assert!(bytes.len() > 1000);
    }

    fn boxed(label: &str, x1: f64, y1: f64, x2: f64, y2: f64) -> Block {
        Block {
            ord: 1,
            label: label.to_string(),
            text: "文字".to_string(),
            md: "文字".to_string(),
            x1,
            y1,
            x2,
            y2,
            poly: None,
        }
    }

    #[test]
    fn detects_two_columns() {
        let mut blocks = vec![boxed("doc_title", 100.0, 20.0, 1100.0, 70.0)];
        for i in 0..4 {
            let y = 100.0 + i as f64 * 60.0;
            blocks.push(boxed("text", 80.0, y, 520.0, y + 40.0));
            blocks.push(boxed("text", 660.0, y, 1120.0, y + 40.0));
        }
        let page = PageDoc {
            page_id: "p1".to_string(),
            w: 1200.0,
            h: 1600.0,
            blocks,
        };
        assert!(page_flow(&page).is_some());
    }

    #[test]
    fn single_column_stays_flat() {
        let mut blocks = Vec::new();
        for i in 0..8 {
            let y = 100.0 + i as f64 * 60.0;
            blocks.push(boxed("text", 80.0, y, 1100.0, y + 40.0));
        }
        let page = PageDoc {
            page_id: "p1".to_string(),
            w: 1200.0,
            h: 1600.0,
            blocks,
        };
        assert!(page_flow(&page).is_none());
    }

    #[test]
    fn parses_html_table() {
        let html = "<table><tr><th>列1</th><th>列2</th></tr><tr><td>a&amp;b</td><td>2</td></tr></table>";
        let rows = parse_table_rows(html);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["列1", "列2"]);
        assert_eq!(rows[1][0], "a&b");
        let page = PageDoc {
            page_id: "p1".to_string(),
            w: 1200.0,
            h: 1600.0,
            blocks: vec![],
        };
        assert!(table_block(html, &page, &|_, _| None).is_some());
    }

    #[test]
    fn parses_markdown_pipe_table() {
        let md = "| a | b |\n| --- | --- |\n| 1 | 2 |\n";
        let rows = parse_md_table_rows(md);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["a", "b"]);
        assert_eq!(rows[1], vec!["1", "2"]);
    }

    #[test]
    fn collects_fig_names_only() {
        let md = "<img src=\"fig_ab12.png\"><img src=\"http://evil/x.png\">";
        assert_eq!(img_srcs(md), vec!["fig_ab12.png".to_string()]);
    }

    #[test]
    fn heading_levels_differ() {
        assert_ne!(format!("{:?}", heading("一", 1)), format!("{:?}", heading("四", 4)));
    }
}
