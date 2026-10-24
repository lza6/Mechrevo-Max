//! 自定义配置的持久化.
//!
//! 存成一个 JSON 文件：`%LOCALAPPDATA%\Mechrevo-Max\profiles.json`。
//! 只用标准库 + serde，不引入数据库（`docs/SPEC.md` §1.2 明确本项目无数据层）。
//!
//! # 容错原则
//!
//! 配置文件**坏了不能让程序起不来**：解析失败就当作"没有配置"，并把损坏文件
//! 重命名成 `profiles.json.broken` 保留现场，然后从空列表继续。

use mechrevo_core::profile::Profile;
use std::path::{Path, PathBuf};

/// 最多允许保存多少条配置（防止配置文件无限膨胀、也避免界面被刷爆）.
pub const MAX_PROFILES: usize = 32;

/// 配置文件路径.
#[must_use]
pub fn profiles_path(dir: &Path) -> PathBuf {
    dir.join("profiles.json")
}

/// 读取配置列表.
///
/// 文件不存在、内容为空、解析失败都返回空列表（不报错）。
/// 解析失败时把原文件改名保留为 `.broken`，避免用户数据被静默覆盖。
#[must_use]
pub fn load_profiles(dir: &Path) -> Vec<Profile> {
    let path = profiles_path(dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    if text.trim().is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<Profile>>(&text) {
        Ok(list) => list.into_iter().filter(|p| p.validate().is_ok()).collect(),
        Err(e) => {
            log::warn!("profiles.json 解析失败，已保留现场: {e}");
            let broken = path.with_extension("json.broken");
            let _ = std::fs::rename(&path, broken);
            Vec::new()
        }
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
    let json = serde_json::to_string_pretty(list).map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(profiles_path(dir), json).map_err(|e| format!("写入配置失败: {e}"))
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

        let loaded = load_profiles(&dir);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "办公静音");
        assert_eq!(loaded[1].pl1, 75);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_yields_empty_list() {
        let dir = temp_dir("missing");
        assert!(load_profiles(&dir).is_empty());
    }

    #[test]
    fn broken_json_is_preserved_not_overwritten() {
        let dir = temp_dir("broken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(profiles_path(&dir), b"{ this is not json ").unwrap();

        let loaded = load_profiles(&dir);
        assert!(loaded.is_empty(), "坏文件应降级为空列表");
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
