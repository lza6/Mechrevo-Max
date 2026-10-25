//! 自定义配置的持久化.
//!
//! 存成一个 JSON 文件：`%LOCALAPPDATA%\Mechrevo-Max\profiles.json`。
//! 只用标准库 + serde，不引入数据库（`docs/SPEC.md` §1.2 明确本项目无数据层）。
//!
//! # 容错原则
//!
//! 文件损坏会保留现场并明确降级；**不会**把损坏或归档失败的文件当作空列表继续覆盖。

use mechrevo_core::profile::Profile;
use std::path::{Path, PathBuf};

/// 最多允许保存多少条配置（防止配置文件无限膨胀、也避免界面被刷爆）.
pub const MAX_PROFILES: usize = 32;

/// 配置文件路径.
#[must_use]
pub fn profiles_path(dir: &Path) -> PathBuf {
    dir.join("profiles.json")
}

/// 配置读取结果（损坏时必须明确降级，不能伪造成“没有配置”）.
pub enum ProfileLoad {
    /// 正常配置列表（包括文件不存在时的空列表）.
    Ready(Vec<Profile>),
    /// 文件存在但已损坏或包含非法条目；用户必须先看提示，禁止静默覆盖.
    NeedsAttention {
        message: String,
        /// 已保留的损坏文件路径；没有备份时为 None.
        backup: Option<PathBuf>,
    },
}

/// 读取配置列表.
///
/// 文件不存在或内容为空返回空列表；任何损坏/非法条目都保留现场，
/// 并尝试归档为 `.broken`。归档失败也会返回错误，禁止继续覆盖。
pub fn load_profiles(dir: &Path) -> ProfileLoad {
    let path = profiles_path(dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ProfileLoad::Ready(Vec::new());
        }
        Err(error) => {
            return ProfileLoad::NeedsAttention {
                message: format!("无法读取配置文件（可能是权限或目录占用）：{error}"),
                backup: None,
            };
        }
    };
    if text.trim().is_empty() {
        return ProfileLoad::Ready(Vec::new());
    }

    let list = match serde_json::from_str::<Vec<Profile>>(&text) {
        Ok(list) => list,
        Err(error) => {
            let backup = path.with_extension("json.broken");
            let archived = archive_broken(&path);
            return ProfileLoad::NeedsAttention {
                message: format!(
                    "配置文件无法解析，已保留原文件：{error}（尝试归档为 profiles.json.broken；{archived}）"
                ),
                backup: backup.exists().then_some(backup),
            };
        }
    };

    let invalid: Vec<String> = list
        .iter()
        .filter(|profile| profile.validate().is_err())
        .map(|profile| profile.name.clone())
        .collect();
    if !invalid.is_empty() {
        return ProfileLoad::NeedsAttention {
            message: format!(
                "配置含非法条目：{}；原文件未修改，请修复后再保存（{}）",
                invalid.join("、"),
                archive_broken(&path)
            ),
            backup: path
                .with_extension("json.broken")
                .exists()
                .then(|| path.with_extension("json.broken")),
        };
    }
    ProfileLoad::Ready(list)
}

/// 尝试保留损坏文件；失败也返回可展示的错误，不掩盖原文件问题.
fn archive_broken(path: &Path) -> String {
    let broken = path.with_extension("json.broken");
    match std::fs::rename(path, &broken) {
        Ok(()) => format!("已保留到 {broken:?}"),
        Err(error) => format!("归档失败：{error}"),
    }
}

/// 覆盖写入配置列表.
///
/// # Errors
/// 任一配置非法、数量超限、或磁盘写入失败时返回错误.
pub fn save_profiles(dir: &Path, list: &[Profile]) -> Result<(), String> {
    if list.len() > MAX_PROFILES {
        return Err(format!("配置最多 {MAX_PROFILES} 条"));
    }
    for profile in list {
        profile.validate().map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("无法创建配置目录 {dir:?}: {e}"))?;
    save_profiles_with(dir, list, replace_file)
}

fn save_profiles_with(
    dir: &Path,
    list: &[Profile],
    replace: impl FnOnce(&Path, &Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let json = serde_json::to_string_pretty(list).map_err(|e| format!("序列化失败: {e}"))?;
    let path = profiles_path(dir);
    let tmp = dir.join("profiles.json.tmp");
    let backup = dir.join("profiles.json.bak");
    {
        let mut file = std::fs::File::create(&tmp).map_err(|e| format!("创建临时配置失败: {e}"))?;
        use std::io::Write as _;
        file.write_all(json.as_bytes())
            .map_err(|e| format!("写入临时配置失败: {e}"))?;
        file.flush().map_err(|e| format!("刷新配置失败: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("同步配置到磁盘失败: {e}"))?;
    }

    match replace(&path, &tmp, &backup) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&tmp);
            Err(format!("替换配置失败，原文件保持不变: {error}"))
        }
    }
}

/// 同目录原子替换；原文件保留为 `.bak`，失败不覆盖旧配置.
#[cfg(windows)]
fn replace_file(target: &Path, replacement: &Path, backup: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        REPLACEFILE_WRITE_THROUGH,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    // 首次创建时 ReplaceFileW 会因目标不存在而失败；此时只需同目录移动临时文件。
    if !target.exists() {
        let result = unsafe {
            MoveFileExW(
                wide(replacement).as_ptr(),
                wide(target).as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        return if result == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        };
    }

    // SAFETY: 三个参数均为以 NUL 结尾的有效 UTF-16 缓冲区，且只在本调用内使用；
    // 传 NULL 给 lpExclude/lpReserved 表示不使用排除列表与保留数据。
    let result = unsafe {
        ReplaceFileW(
            wide(target).as_ptr(),
            wide(replacement).as_ptr(),
            wide(backup).as_ptr(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(target: &Path, replacement: &Path, backup: &Path) -> std::io::Result<()> {
    if target.exists() {
        std::fs::copy(target, backup)?;
    }
    std::fs::rename(replacement, target)
}

/// 从已保留的损坏文件恢复配置（显式用户操作，不自动清空）.
///
/// 只接受当前数据目录下的 `profiles.json.broken`，避免把任意文件当配置恢复。
/// 恢复前会把当前 `profiles.json` 保存为 `.bak`；解析与校验失败不修改目标。
pub fn restore_broken_profiles(dir: &Path) -> Result<Vec<Profile>, String> {
    let backup = profiles_path(dir).with_extension("json.broken");
    let text = std::fs::read_to_string(&backup)
        .map_err(|e| format!("找不到可恢复的配置备份 {backup:?}: {e}"))?;
    let list: Vec<Profile> = serde_json::from_str(&text)
        .map_err(|e| format!("备份仍不是有效配置，未修改当前文件: {e}"))?;
    for profile in &list {
        profile
            .validate()
            .map_err(|e| format!("备份含非法配置，未恢复: {e}"))?;
    }
    save_profiles(dir, &list)?;
    Ok(list)
}

/// 插入或覆盖同名配置（同名视为更新）.
///
/// # Errors
/// 校验失败或超限时返回错误.
pub fn upsert_profile(list: &mut Vec<Profile>, profile: Profile) -> Result<(), String> {
    profile.validate().map_err(|e| e.to_string())?;
    match list.iter_mut().find(|p| p.name == profile.name) {
        Some(slot) => *slot = profile,
        None => {
            if list.len() >= MAX_PROFILES {
                return Err(format!("配置最多 {MAX_PROFILES} 条，请先删除一些"));
            }
            list.push(profile);
        }
    }
    Ok(())
}

/// 按名字删除；返回是否真的删掉了.
pub fn remove_profile(list: &mut Vec<Profile>, name: &str) -> bool {
    let before = list.len();
    list.retain(|p| p.name != name);
    list.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("mechrevo-prof-{tag}-{}-{n}", std::process::id()))
    }

    fn sample(name: &str, pl1: u8) -> Profile {
        Profile::new(name, pl1, 45, 200, false).unwrap()
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = temp_dir("roundtrip");
        let list = vec![sample("办公静音", 45), sample("游戏", 75)];
        save_profiles(&dir, &list).unwrap();

        let loaded = match load_profiles(&dir) {
            ProfileLoad::Ready(list) => list,
            ProfileLoad::NeedsAttention { message: error, .. } => {
                panic!("损坏文件不应被静默覆盖: {error}")
            }
        };
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "办公静音");
        assert_eq!(loaded[1].pl1, 75);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_yields_empty_list() {
        let dir = temp_dir("missing");
        assert!(matches!(load_profiles(&dir), ProfileLoad::Ready(list) if list.is_empty()));
    }

    #[test]
    fn broken_json_is_preserved_not_overwritten() {
        let dir = temp_dir("broken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(profiles_path(&dir), b"{ this is not json ").unwrap();

        let ProfileLoad::NeedsAttention { message, .. } = load_profiles(&dir) else {
            panic!("损坏文件必须进入 NeedsAttention，而不是空列表");
        };
        assert!(message.contains("无法解析"), "实际: {message}");
        assert!(
            profiles_path(&dir).with_extension("json.broken").exists(),
            "坏文件必须改名保留现场"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_replaces_same_name_and_removes() {
        let mut list = vec![sample("游戏", 60)];
        upsert_profile(&mut list, sample("游戏", 80)).unwrap();
        assert_eq!(list.len(), 1, "同名应覆盖而不是追加");
        assert_eq!(list[0].pl1, 80);

        assert!(remove_profile(&mut list, "游戏"));
        assert!(list.is_empty());
        assert!(!remove_profile(&mut list, "不存在"));
    }

    #[test]
    fn invalid_entry_requires_attention_without_losing_original() {
        let dir = temp_dir("invalid-entry");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            profiles_path(&dir),
            r#"[{"name":"合法","pl1":45,"pl2":45,"pl4":200,"fan_boost":false},{"name":"非法","pl1":1,"pl2":45,"pl4":200,"fan_boost":false}]"#,
        )
        .unwrap();

        let ProfileLoad::NeedsAttention { message, .. } = load_profiles(&dir) else {
            panic!("非法条目必须进入 NeedsAttention");
        };
        assert!(message.contains("非法条目"));
        assert!(message.contains("已保留"));
        assert!(!profiles_path(&dir).exists(), "原件应被归档，而非覆盖");
        assert!(profiles_path(&dir).with_extension("json.broken").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_creates_backup_of_previous_valid_file() {
        let dir = temp_dir("backup");
        save_profiles(&dir, &[sample("old", 45)]).unwrap();
        save_profiles(&dir, &[sample("new", 75)]).unwrap();

        let backup = std::fs::read_to_string(dir.join("profiles.json.bak")).unwrap();
        assert!(backup.contains("old"), "替换前配置必须保留在 .bak");
        let current = std::fs::read_to_string(profiles_path(&dir)).unwrap();
        assert!(current.contains("new"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replacement_failure_preserves_original_bytes() {
        let dir = temp_dir("replace-failure");
        save_profiles(&dir, &[sample("old", 45)]).unwrap();
        let original = std::fs::read(profiles_path(&dir)).unwrap();

        let error = save_profiles_with(&dir, &[sample("new", 75)], |_, _, _| {
            Err(std::io::Error::other("injected replace failure"))
        })
        .unwrap_err();
        assert!(error.contains("原文件保持不变"));
        assert_eq!(std::fs::read(profiles_path(&dir)).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_valid_backup_returns_profiles_and_preserves_current_backup() {
        let dir = temp_dir("restore");
        std::fs::create_dir_all(&dir).unwrap();
        save_profiles(&dir, &[sample("current", 45)]).unwrap();
        std::fs::write(
            profiles_path(&dir).with_extension("json.broken"),
            r#"[{"name":"restored","pl1":60,"pl2":45,"pl4":200,"fan_boost":false}]"#,
        )
        .unwrap();

        let restored = restore_broken_profiles(&dir).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].name, "restored");
        assert!(std::fs::read_to_string(dir.join("profiles.json.bak"))
            .unwrap()
            .contains("current"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_backup_is_not_restored() {
        let dir = temp_dir("bad-restore");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            profiles_path(&dir).with_extension("json.broken"),
            b"not json",
        )
        .unwrap();
        assert!(restore_broken_profiles(&dir).is_err());
        assert!(!profiles_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_too_many_profiles() {
        let dir = temp_dir("limit");
        let list: Vec<Profile> = (0..=MAX_PROFILES)
            .map(|i| Profile::new(format!("p{i}"), 45, 45, 200, false).unwrap())
            .collect();
        assert!(save_profiles(&dir, &list).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_invalid_profile_on_save() {
        let dir = temp_dir("invalid");
        // 绕过构造校验，模拟被外部篡改的数据
        let bad = Profile {
            name: "x".into(),
            pl1: 1,
            pl2: 45,
            pl4: 200,
            fan_boost: false,
        };
        assert!(save_profiles(&dir, &[bad]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
