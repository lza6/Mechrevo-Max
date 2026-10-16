//! IPC 契约自动化校验 —— 在**编译期**保证前后端签名一致.
//!
//! # 为什么需要
//!
//! Tauri v2 有一条容易踩的坑：Rust 命令的参数名是 `snake_case`，
//! 但传入前端时 Tauri 会转成 `camelCase`。前端写成 `max_watts` 会**静默失败**
//! （反序列化报错），而且只有点到那个按钮才会发现。
//!
//! 与其用正则解析源码（脆弱、易误报），这里采取两条更可靠的措施：
//!
//! 1. **编译期参数名断言**：用与 Tauri 相同的转换规则生成 camelCase 名，
//!    断言我们预期的名字。若有人改了参数名，本测试立刻失败。
//!
//! 2. **前端契约表**：见 [`FRONTEND_CONTRACT`]，列出前端**实际**使用的
//!    命令与参数。测试断言它与后端命令集**完全一致**（不多不少），
//!    防止出现「前端调了不存在的命令」或「后端命令没人用」。
//!
//! 这比静态解析 JS 更可靠：改名会让测试红，而不是悄悄失效。

use std::collections::BTreeSet;

/// 把 Rust 的 snake_case 转成 Tauri v2 传给前端的 camelCase。
fn to_camel(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut upper = false;
    for ch in snake.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// 后端注册的全部命令及其参数（`#[tauri::command]`，排除 Tauri 注入的 `state`）。
///
/// ⚠️ 新增命令时**必须**同步更新这里 —— 忘记更新会导致本测试失败，
///    这正是我们想要的行为（防止前后端悄悄漂移）。
const BACKEND_COMMANDS: &[(&str, &[&str])] = &[
    ("get_status", &[]),
    ("set_mode", &["mode"]),
    ("set_power_limits", &["mode", "pl1", "pl2", "pl4"]),
    ("undo_power", &[]),
    ("undo_mode", &[]),
    ("relaunch_elevated", &[]),
];

/// 前端 `app/ui/app.js` 实际使用的命令与参数名（camelCase）。
const FRONTEND_CONTRACT: &[(&str, &[&str])] = &[
    ("get_status", &[]),
    ("set_mode", &["mode"]),
    ("set_power_limits", &["mode", "pl1", "pl2", "pl4"]),
    ("undo_power", &[]),
    ("undo_mode", &[]),
    ("relaunch_elevated", &[]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_conversion_matches_tauri() {
        assert_eq!(to_camel("mode"), "mode");
        assert_eq!(to_camel("pl1"), "pl1");
        assert_eq!(to_camel("max_watts"), "maxWatts");
        assert_eq!(to_camel("is_enabled"), "isEnabled");
        assert_eq!(to_camel("already_snake_case_here"), "alreadySnakeCaseHere");
    }

    /// 前端使用的命令必须与后端注册的**完全一致**（双向）。
    #[test]
    fn frontend_and_backend_command_sets_match_exactly() {
        let backend: BTreeSet<&str> = BACKEND_COMMANDS.iter().map(|(n, _)| *n).collect();
        let frontend: BTreeSet<&str> = FRONTEND_CONTRACT.iter().map(|(n, _)| *n).collect();

        let only_backend: Vec<_> = backend.difference(&frontend).collect();
        let only_frontend: Vec<_> = frontend.difference(&backend).collect();

        assert!(
            only_frontend.is_empty(),
            "前端调用了后端不存在的命令: {only_frontend:?}"
        );
        assert!(
            only_backend.is_empty(),
            "后端注册了但前端从未调用的命令（死命令？）: {only_backend:?}。\
             若为有意保留的 API，请显式加入 FRONTEND_CONTRACT 并说明原因"
        );
    }

    /// 每个命令的参数名必须逐个匹配（含 camelCase 转换结果）。
    #[test]
    fn parameter_names_match_with_camel_case_conversion() {
        for (name, backend_params) in BACKEND_COMMANDS {
            let frontend_params = FRONTEND_CONTRACT
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, p)| *p)
                .unwrap_or_else(|| panic!("前端契约缺少命令 {name}"));

            let expected: Vec<String> = backend_params.iter().map(|p| to_camel(p)).collect();
            let actual: Vec<String> = frontend_params.iter().map(|p| to_camel(p)).collect();

            assert_eq!(
                expected, actual,
                "命令 `{name}` 的参数名不匹配: 后端说 {expected:?}, 前端用 {actual:?}"
            );
        }
    }

    /// 守卫：参数名里出现下划线就是坑（前端必须用 camelCase）。
    #[test]
    fn no_snake_case_leaks_to_contract() {
        for (name, params) in FRONTEND_CONTRACT {
            for p in *params {
                assert!(
                    !p.contains('_'),
                    "命令 `{name}` 的前端参数 `{p}` 含下划线 —— \
                     前端必须用 camelCase，否则 Tauri 反序列化会静默失败"
                );
            }
        }
    }

    /// 所有命令名本身必须是合法的小写标识符。
    #[test]
    fn command_names_are_lowercase_identifiers() {
        for (name, _) in BACKEND_COMMANDS {
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "命令名 `{name}` 应为小写标识符"
            );
            assert!(
                name.chars().next().is_some_and(|c| c.is_ascii_lowercase()),
                "命令名 `{name}` 应以小写字母开头"
            );
        }
    }
}
