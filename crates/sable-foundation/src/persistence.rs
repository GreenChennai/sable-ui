//! 原子写与自动保存(docs/06 §1.2):崩溃不丢稿的底线。
//!
//! 源自项目设计文档 docs/06 §1.2。配套铁律:文档模型必须随时可完整序列化
//! (scene.rs 的 serde 覆盖测试即为该铁律的验收)。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 原子写:先写临时文件,fsync,再 rename —— 写到一半断电也不会得到半个文件。
///
/// `std::fs::rename` 在 Windows 上走 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`
/// 语义,可直接覆盖已存在的目标文件;POSIX 上为原子替换。临时文件与目标同目录,
/// 不存在跨卷问题。
pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("lumi.tmp");
    let write_result = (|| {
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(data)?;
            f.sync_all()?; // 落盘
        }
        fs::rename(&tmp, path)
    })();
    if write_result.is_err() {
        // 失败时清掉残留的 .tmp,不留垃圾
        let _ = fs::remove_file(&tmp);
    }
    write_result
}

/// 自动保存目录:`%APPDATA%/{app}/autosave`(Windows);Unix 用
/// `$XDG_DATA_HOME` 或 `~/.local/share/{app}/autosave`。只拼路径不建目录,
/// 目录在首次写入时创建(见 [`AutosavePlan::write`])。
pub fn autosave_dir(app_name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(|home| PathBuf::from(home).join("AppData\\Roaming"))
            })
            .unwrap_or_else(std::env::temp_dir);
        base.join(app_name).join("autosave")
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(std::env::temp_dir);
        base.join(app_name).join("autosave")
    }
}

/// 自动保存计划:保存目录 + 轮换保留份数。
///
/// 崩溃后下次启动检测 autosave 目录 → 弹"恢复未保存的工作?"(docs/06 §1.2)。
#[derive(Clone, Debug)]
pub struct AutosavePlan {
    /// 自动保存目录
    pub dir: PathBuf,
    /// 轮换时最多保留的文件份数(`keep = 0` 表示清空)
    pub keep: usize,
}

impl AutosavePlan {
    /// 以 [`autosave_dir`] 为目录构造计划。
    pub fn new(app_name: &str, keep: usize) -> Self {
        AutosavePlan {
            dir: autosave_dir(app_name),
            keep,
        }
    }

    /// 某个工程的自动保存文件路径:`{dir}/{project_id}.sable`。
    pub fn path_for(&self, project_id: &str) -> PathBuf {
        self.dir.join(format!("{project_id}.sable"))
    }

    /// 原子写一份自动保存(目录不存在则先建)。
    pub fn write(&self, project_id: &str, data: &[u8]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        atomic_write(&self.path_for(project_id), data)
    }

    /// 轮换:按修改时间从新到旧保留 [`Self::keep`] 份,其余删除。
    /// 目录不存在视为无可轮换,返回 `Ok(())`。
    pub fn rotate(&self) -> io::Result<()> {
        if self.keep == 0 {
            return Ok(());
        }
        if !self.dir.exists() {
            return Ok(());
        }
        let mut entries: Vec<(SystemTime, PathBuf)> = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let modified = entry.metadata()?.modified()?;
                entries.push((modified, path));
            }
        }
        // 新 → 旧排序;同刻按路径名稳定排序,保证可复现
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        for (_, path) in entries.into_iter().skip(self.keep) {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试独占一个临时目录,先清历史再建
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sable-foundation-persist-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    #[test]
    fn atomic_write_creates_file_with_exact_content() {
        let dir = temp_dir("write");
        let path = dir.join("doc.sable");
        atomic_write(&path, b"hello sable").expect("首次写入");
        assert_eq!(fs::read(&path).expect("读回"), b"hello sable");
        // 落盘后不留 .tmp 残留
        let tmp = path.with_extension("lumi.tmp");
        assert!(!tmp.exists(), "rename 成功后临时文件应消失");
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let dir = temp_dir("overwrite");
        let path = dir.join("doc.sable");
        atomic_write(&path, b"v1").expect("第一版");
        atomic_write(&path, b"version-2-longer").expect("覆盖写");
        assert_eq!(fs::read(&path).expect("读回"), b"version-2-longer");
    }

    #[test]
    fn atomic_write_failure_leaves_original_intact() {
        let dir = temp_dir("fail");
        let path = dir.join("doc.sable");
        atomic_write(&path, b"good").expect("先写一份");
        // 目标"目录"当文件用 → create 失败 → 原文件不受影响
        let bad = dir.join("sub");
        fs::create_dir_all(&bad).expect("目录占位");
        let result = atomic_write(&bad, b"boom");
        assert!(result.is_err(), "写目录路径应失败");
        assert_eq!(fs::read(&path).expect("原文件仍在"), b"good");
        assert!(!dir.join("sub.sable.tmp").exists(), "失败后无 .tmp 残留");
    }

    #[test]
    fn autosave_plan_write_rotate_keeps_newest() {
        let plan = AutosavePlan {
            dir: temp_dir("rotate"),
            keep: 2,
        };

        for i in 0..5u32 {
            plan.write(&format!("p{i}"), format!("data-{i}").as_bytes())
                .expect("自动保存");
            // mtime 粒度可能粗于写入间隔,显式等一下保证新旧可辨
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        plan.rotate().expect("轮换");
        let mut left: Vec<String> = fs::read_dir(&plan.dir)
            .expect("读目录")
            .map(|e| e.expect("项").file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left.len(), 2, "轮换后只保留 keep 份,实际 {left:?}");
        assert_eq!(left, vec!["p3.sable", "p4.sable"], "保留最新两份");
    }

    #[test]
    fn autosave_rotate_on_missing_dir_is_ok() {
        let plan = AutosavePlan {
            dir: temp_dir("missing").join("不存在的子目录"),
            keep: 3,
        };
        plan.rotate().expect("目录不存在应 Ok");
    }

    #[test]
    fn autosave_dir_layout_contains_app_name() {
        let dir = autosave_dir("SableTest");
        let as_str = dir.to_string_lossy().into_owned();
        assert!(as_str.contains("SableTest"), "包含应用名: {as_str}");
        assert!(as_str.ends_with("autosave"), "以 autosave 结尾: {as_str}");
    }
}
