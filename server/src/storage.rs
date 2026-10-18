//! 数据目录布局与原子写。

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// `{data_dir}/g/{group_id}/{page_id}/`
pub fn page_dir(data_dir: &Path, group_id: &str, page_id: &str) -> PathBuf {
    data_dir.join("g").join(group_id).join(page_id)
}

/// 页内的具名文件：`raw.jpg` / `edit.jpg` / `disp.jpg` / `thumb.jpg` / `blocks.json`
pub fn page_file(data_dir: &Path, group_id: &str, page_id: &str, name: &str) -> PathBuf {
    page_dir(data_dir, group_id, page_id).join(name)
}

/// `{data_dir}/g/{group_id}/docx/{rev}.docx`
pub fn docx_path(data_dir: &Path, group_id: &str, rev: i64) -> PathBuf {
    data_dir
        .join("g")
        .join(group_id)
        .join("docx")
        .join(format!("{rev}.docx"))
}

/// 原子写：先写临时文件再 rename，避免半截文件被读到。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::internal(format!("路径无父目录: {}", path.display())))?;
    std::fs::create_dir_all(parent).map_err(AppError::internal)?;

    let tmp = path.with_extension(format!(
        "tmp-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&tmp, bytes).map_err(AppError::internal)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        AppError::internal(e)
    })
}

/// 校验 id 只含安全字符，防目录穿越。
pub fn safe_id(id: &str) -> AppResult<&str> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        Ok(id)
    } else {
        Err(AppError::BadRequest("非法的 id".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        assert!(safe_id("../etc/passwd").is_err());
        assert!(safe_id("a/b").is_err());
        assert!(safe_id("").is_err());
        assert!(safe_id("9f1c2b7e-0000-4000-8000-000000000000").is_ok());
    }

    #[test]
    fn atomic_write_creates_parents() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a").join("b").join("c.jpg");
        write_atomic(&target, b"hello").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
    }
}
