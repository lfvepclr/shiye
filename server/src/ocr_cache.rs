//! 识别结果缓存：字节相同的图片不重复调用百度 OCR（按次计费）。
//!
//! 键为页面 `edit.jpg` 的 SHA-256，值为「适配后的块 + 页内插图」：
//! `{data_dir}/cache/ocr/{VERSION}-{sha256}/result.json` 与 `figs/`。
//!
//! 命中时不碰网络：插图复制回页面目录，块直接入库，`ocr_ms` 只反映本地耗时，
//! 因此从 `pages.ocr_ms` 也能看出这次是缓存命中。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::blocks::Block;
use crate::error::{AppError, AppResult};
use crate::storage;

/// 缓存格式版本。百度参数或适配逻辑一变就加 1，旧缓存自然失效。
pub const VERSION: &str = "v1";

/// 最多保留多少张图的缓存，超出按最近使用时间淘汰；单页约几百 KB。
pub const MAX_ENTRIES: usize = 500;

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    version: String,
    width: u32,
    height: u32,
    blocks: Vec<Block>,
}

/// 一次缓存命中：块数据 + 缓存目录（插图来源）。
pub struct Hit {
    pub width: u32,
    pub height: u32,
    pub blocks: Vec<Block>,
    dir: PathBuf,
}

/// 图片内容指纹。同一张图无论重传、重排、换组，键都一致。
pub fn key(image: &[u8]) -> String {
    format!("{VERSION}-{}", hex::encode(Sha256::digest(image)))
}

/// 读缓存；任何异常都当未命中（缓存坏了不能挡住正常识别）。
pub fn load(data_dir: &Path, key: &str) -> Option<Hit> {
    let dir = storage::ocr_cache_dir(data_dir).join(key);
    let raw = std::fs::read(dir.join("result.json")).ok()?;
    let entry: Entry = serde_json::from_slice(&raw).ok()?;
    if entry.version != VERSION {
        return None;
    }
    // 命中即刷新 mtime，给淘汰策略一个「最近使用」信号。
    let _ = std::fs::File::open(dir.join("result.json"))
        .and_then(|f| f.set_modified(std::time::SystemTime::now()));

    Some(Hit {
        width: entry.width,
        height: entry.height,
        blocks: entry.blocks,
        dir,
    })
}

/// 把缓存里的插图复制回页面的 `fig/` 目录（块里的 `<img src="fig_x.png">` 依赖这些文件）。
pub fn materialize(hit: &Hit, fig_dir: &Path) -> AppResult<()> {
    let figs = hit.dir.join("figs");
    let Ok(entries) = std::fs::read_dir(&figs) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // 只搬 fig_<hex>.<png|jpg>，杜绝路径穿越
        if crate::blocks::sanitize_fig_name(name).is_none() {
            continue;
        }
        let bytes = std::fs::read(entry.path()).map_err(AppError::internal)?;
        storage::write_atomic(&fig_dir.join(name), &bytes)?;
    }
    Ok(())
}

/// 写缓存：先写临时目录再整体 rename，避免半截缓存被读到。
pub fn save(
    data_dir: &Path,
    key: &str,
    width: u32,
    height: u32,
    blocks: &[Block],
    fig_dir: &Path,
) -> AppResult<()> {
    let root = storage::ocr_cache_dir(data_dir);
    std::fs::create_dir_all(&root).map_err(AppError::internal)?;

    let tmp = root.join(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(tmp.join("figs")).map_err(AppError::internal)?;

    let entry = Entry {
        version: VERSION.to_string(),
        width,
        height,
        blocks: blocks.to_vec(),
    };
    let json = serde_json::to_vec(&entry)
        .map_err(|e| AppError::internal(format!("序列化识别缓存失败: {e}")))?;
    std::fs::write(tmp.join("result.json"), json).map_err(AppError::internal)?;

    for name in collect_figs(blocks) {
        let src = fig_dir.join(&name);
        if !src.exists() {
            continue;
        }
        let bytes = std::fs::read(&src).map_err(AppError::internal)?;
        std::fs::write(tmp.join("figs").join(&name), bytes).map_err(AppError::internal)?;
    }

    let target = root.join(key);
    if target.exists() {
        let _ = std::fs::remove_dir_all(&target);
    }
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp);
        AppError::internal(e)
    })?;

    prune(&root);
    Ok(())
}

/// 块里引用到的插图文件名（表格内嵌图同样以 `fig_*` 命名）。
fn collect_figs(blocks: &[Block]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for b in blocks {
        let mut rest = b.md.as_str();
        while let Some(pos) = rest.find("fig_") {
            rest = &rest[pos..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                .unwrap_or(rest.len());
            if let Some(name) = crate::blocks::sanitize_fig_name(&rest[..end]) {
                out.insert(name);
            }
            rest = &rest[end..];
        }
    }
    out
}

/// 超过 `MAX_ENTRIES` 时删最久没被用过的条目。
fn prune(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| !e.file_name().to_string_lossy().starts_with(".tmp-"))
        .filter_map(|e| {
            let mtime = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((mtime, e.path()))
        })
        .collect();

    if dirs.len() <= MAX_ENTRIES {
        return;
    }
    dirs.sort_by(|a, b| b.0.cmp(&a.0)); // 新 → 旧
    for (_, path) in dirs.into_iter().skip(MAX_ENTRIES) {
        let _ = std::fs::remove_dir_all(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(md: &str) -> Block {
        Block {
            ord: 1,
            label: "text".into(),
            text: "正文".into(),
            md: md.into(),
            x1: 1.0,
            y1: 2.0,
            x2: 30.0,
            y2: 40.0,
            poly: None,
        }
    }

    #[test]
    fn key_is_content_addressed() {
        let a = key(b"same image bytes");
        assert_eq!(a, key(b"same image bytes"));
        assert_ne!(a, key(b"same image bytez"));
        assert!(a.starts_with("v1-"));
        assert_eq!(a.len(), 3 + 64);
    }

    #[test]
    fn save_and_load_roundtrip_with_figures() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        let page_dir = data_dir.join("g/g1/p1");
        std::fs::create_dir_all(&page_dir).unwrap();
        std::fs::write(page_dir.join("fig_ab12.png"), b"png-bytes").unwrap();

        let blocks = vec![block("<img src=\"fig_ab12.png\">")];
        let k = key(b"image");
        save(data_dir, &k, 1200, 1600, &blocks, &page_dir).unwrap();

        let hit = load(data_dir, &k).expect("应当命中");
        assert_eq!((hit.width, hit.height), (1200, 1600));
        assert_eq!(hit.blocks.len(), 1);

        let target = data_dir.join("g/g1/p2");
        std::fs::create_dir_all(&target).unwrap();
        materialize(&hit, &target).unwrap();
        assert_eq!(
            std::fs::read(target.join("fig_ab12.png")).unwrap(),
            b"png-bytes"
        );
    }

    #[test]
    fn load_misses_when_version_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let k = key(b"image");
        let dir = storage::ocr_cache_dir(tmp.path()).join(&k);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("result.json"),
            r#"{"version":"v0","width":1,"height":1,"blocks":[]}"#,
        )
        .unwrap();
        assert!(load(tmp.path(), &k).is_none());
    }

    #[test]
    fn prune_keeps_newest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = storage::ocr_cache_dir(tmp.path());
        std::fs::create_dir_all(&root).unwrap();
        // 直接造 MAX_ENTRIES + 1 个目录，看是否只删 1 个
        for i in 0..=MAX_ENTRIES {
            std::fs::create_dir_all(root.join(format!("v1-{i:064x}"))).unwrap();
        }
        prune(&root);
        let count = std::fs::read_dir(&root).unwrap().count();
        assert_eq!(count, MAX_ENTRIES);
    }
}
