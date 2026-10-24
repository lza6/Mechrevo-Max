# Skill: 发布清单

## 适用场景
要打 tag / 建 Release / 宣布新版本时。

## 前置读取
`docs/DEPLOYMENT.md`（§2 构建坑、§4 版本规则、§7 清单）、`CHANGELOG.md`。

## 步骤
1. 全绿：`cd core && cargo test && cargo clippy --all-targets && cargo fmt --check`
2. 应用：`cd app/src-tauri && cargo test --bin mechrevo-max && cargo test --test ipc_contract`
3. 构建：`export PATH="/c/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64:$PATH" && cargo build --release`（**6–8 分钟，勿误判卡死**）
4. 三处版本号同步：`core/Cargo.toml`、`app/src-tauri/Cargo.toml`、`tauri.conf.json`
5. `CHANGELOG.md` 新段（数字必须与事实一致：测试数、工具数、文件数）
6. 合规自检：`git ls-files | grep -Ei "\.(sys|msix|msixbundle|dll|exe)$"` 输出必须为空
7. `git add -A && git commit` → `git tag -a vX.Y.Z` → `git push origin main && git push origin vX.Y.Z`
8. `python -X utf8 -I tools/publish_release.py vX.Y.Z --title "..." --notes-file CHANGELOG.md`

## 验收标准
Release 页面有产物 + notes；`docs/VERIFICATION-LEDGER.md` 已更新时间戳；`project_specs.md` 状态已更新。

## 证据要求
exe 大小 + SHA256；远端 tag 的 `git ls-remote` 输出；Release URL。

## 回滚方式
用户下载上一版 tag 的 exe 覆盖即可（绿色单文件，无迁移）。**禁止 force push**。

## 常见坑
- 改 `app/ui/*` 后必须 `touch app/src-tauri/src/main.rs` 再构建，否则前端资源不重嵌入
- 本机可能无外网访问 `api.github.com`；此时 Release 只能标"待验证"，不得声称已发布
