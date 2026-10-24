//! 真机集成测试 —— 直接操作本机 EC.
//!
//! 这些测试**默认跳过**（`#[ignore]`），因为它们需要：
//! - Windows 管理员权限
//! - 已加载 `UWACPIDriver` 且设备已枚举
//! - 真实硬件
//!
//! 显式运行：
//! ```bash
//! cargo test --test hardware -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! # 安全说明
//!
//! 写测试遵循「先存回滚点 → 写入 → 读回校验 → 立即还原」的流程，
//! 测试结束后硬件状态应完全恢复原样。
//!
//! **所有写操作都必须挂在 [`RestoreGuard`] 上**：断言失败会让测试 panic 展开、
//! 跳过后续语句，只有 `Drop` 仍会执行。没有守卫时，一次断言失败就会把硬件
//! 停在改动后的状态（见 `docs/计划书/下一步改进指南.md` P0-01）。

use mechrevo_core::ec::{Ec, WindowsAcpiTransport};
use mechrevo_core::namespace::Mode;
use mechrevo_core::{PowerController, PowerLimits, Sensors};

fn ec() -> Ec<WindowsAcpiTransport> {
    // WindowsAcpiTransport 内部强制限速（默认 50 次/秒），此处拿到的就是安全通路
    let transport = WindowsAcpiTransport::new().expect("无法构造传输层 —— EC 访问仅支持 Windows");
    Ec::new(transport)
}

#[test]
#[ignore = "需要真机 + 管理员权限 + 驱动就绪"]
fn device_is_reachable() {
    let transport = WindowsAcpiTransport::new().unwrap();
    match transport.probe() {
        Ok(()) => println!("✅ EC 设备可访问: {}", transport.path()),
        Err(e) => panic!("❌ EC 设备不可访问:\n{e}"),
    }
}

#[test]
#[ignore = "需要真机 + 管理员权限"]
fn reads_verified_project_id() {
    let ec = ec();
    let id = ec.read(0x0740).expect("读取项目 ID 失败");
    println!("项目 ID = {id}");
    assert_eq!(id, 16, "本机项目 ID 应为 16 (官方注册表 ProjectID)");
}

#[test]
#[ignore = "需要真机 + 管理员权限"]
fn reads_sensors_in_plausible_range() {
    let ec = ec();
    let sensors = Sensors::new(&ec);
    let snap = sensors.snapshot().expect("读取传感器失败");
    println!("传感器快照: {snap:#?}");

    assert_eq!(snap.project_id, 16);
    assert!(
        snap.cpu_fan_rpm < 10_000,
        "CPU 风扇转速 {} 超出合理范围",
        snap.cpu_fan_rpm
    );
    assert!(
        snap.gpu_fan_rpm < 10_000,
        "GPU 风扇转速 {} 超出合理范围",
        snap.gpu_fan_rpm
    );
}

#[test]
#[ignore = "需要真机 + 管理员权限"]
fn reads_official_power_defaults() {
    let ec = ec();
    let ctl = PowerController::new(&ec);

    let gaming = ctl
        .read_defaults(Mode::Gaming)
        .expect("读取 Gaming 功耗失败");
    let office = ctl
        .read_defaults(Mode::Office)
        .expect("读取 Office 功耗失败");
    println!("Gaming: {gaming:?}");
    println!("Office: {office:?}");

    assert_eq!(
        (gaming.pl1, gaming.pl2, gaming.pl4),
        (75, 75, 200),
        "Gaming 默认功耗应与官方注册表一致"
    );
    assert_eq!(
        (office.pl1, office.pl2, office.pl4),
        (45, 45, 200),
        "Office 默认功耗应与官方注册表一致"
    );
}

#[test]
#[ignore = "需要真机 + 管理员权限"]
fn write_same_value_is_a_noop() {
    // 最保守的写验证: 把当前值原样写回, 确认写通道通且读回一致
    let ec = ec();
    let original = ec.read(0x0730).expect("读取 PL1 失败");
    ec.write_verified(0x0730, original)
        .expect("写回原值失败 —— 写通道可能不通");
    assert_eq!(ec.read(0x0730).unwrap(), original);
    println!("✅ 写通道可用 (回写原值 {original} 成功)");
}

#[test]
#[ignore = "需要真机 + 管理员权限, 会短暂改变硬件设置"]
fn real_write_then_restore() {
    // 真实功能写入: 改 Office 默认 PL1, 验证生效, 然后立即还原
    let ec = ec();
    let ctl = PowerController::new(&ec);

    let before = ctl.read_defaults(Mode::Office).expect("读取原始值失败");
    println!("改动前 Office: {before:?}");

    let target = PowerLimits::new(before.pl1.saturating_add(5), before.pl2, before.pl4)
        .expect("构造目标值失败");
    println!("目标 Office: {target:?}");

    let record = ctl.apply_defaults(Mode::Office, &target).expect("写入失败");

    // 从这里开始硬件已被改动 —— 守卫保证 panic 时也会还原
    let mut guard = ec.guard(record.rollback.clone());

    let after = ctl.read_defaults(Mode::Office).expect("读回失败");
    println!("改动后 Office: {after:?}");
    assert_eq!(after.pl1, target.pl1, "PL1 应已生效");

    // 立即还原
    let restored = guard.restore_now().expect("回滚失败");
    println!("回滚 {restored} 个寄存器");

    let final_state = ctl.read_defaults(Mode::Office).expect("最终读取失败");
    println!("还原后 Office: {final_state:?}");
    assert_eq!(final_state, before, "必须完全恢复原状");
}

#[test]
#[ignore = "需要真机 + 管理员权限, 会短暂改变强冷位"]
fn fan_boost_roundtrip() {
    // 强冷 = 0x751 的 bit6。这里做「读原值 → 翻转 → 校验 → 翻转回 → 校验复原」，
    // 全程带 snapshot + RAII 守卫，任何一步失败都会还原。
    use mechrevo_core::{ModeController, OperatingMode};

    let ec = ec();
    let ctl = ModeController::new(&ec);

    let original = ctl.fan_boost().expect("读取强冷状态失败");
    let (f1, f2) = ctl.raw_fields().expect("读取档位字段失败");
    println!("当前强冷 = {original} (751=0x{f1:02X}, 7AB=0x{f2:02X})");

    let change = ctl.set_fan_boost(!original).expect("翻转强冷失败");
    let mut guard = ec.guard(change.rollback.clone());
    let flipped = ctl.fan_boost().expect("读回失败");
    println!(
        "翻转后强冷 = {flipped} (751=0x{:02X})",
        ctl.raw_fields().unwrap().0
    );
    assert_eq!(flipped, !original, "强冷位未按预期翻转");

    // 档位编码必须保持不变（只动 bit6）
    assert_eq!(
        ctl.current().expect("读回档位失败"),
        OperatingMode::from_field1(f1),
        "翻转强冷不应改变档位编码"
    );

    let restored = guard.restore_now().expect("回滚失败");
    println!("回滚 {restored} 个寄存器");

    let (n1, n2) = ctl.raw_fields().expect("最终读取失败");
    println!("最终 751=0x{n1:02X}, 7AB=0x{n2:02X}");
    assert_eq!((n1, n2), (f1, f2), "0x751/0x7AB 必须完全复原");
}

#[test]
#[ignore = "需要真机 + 管理员权限, 会短暂改变风扇/功耗档位"]
fn mode_switch_roundtrip() {
    use mechrevo_core::{ModeController, OperatingMode};

    let ec = ec();
    let ctl = ModeController::new(&ec);

    let original = ctl.current().expect("读取当前档位失败");
    let (f1, f2) = ctl.raw_fields().expect("读取档位字段失败");
    println!("当前档位: {original:?}  (751=0x{f1:02X}, 7AB=0x{f2:02X})");
    assert!(original.is_some(), "应当能识别出当前档位");

    let sensors = Sensors::new(&ec);

    for mode in OperatingMode::all() {
        let change = ctl.switch(mode).expect("切换失败");
        // 档位已变 —— 守卫确保断言失败/panic 时也会还原
        let mut guard = ec.guard(change.rollback.clone());

        let after = ctl.current().expect("读回失败");
        let snap = sensors.snapshot().expect("读传感器失败");
        println!(
            "切换到 {}: 读回 {after:?}, CPU 风扇 {} RPM, GPU 风扇 {} RPM",
            mode.display_name(),
            snap.cpu_fan_rpm,
            snap.gpu_fan_rpm
        );
        assert_eq!(after, Some(mode), "切换后应能读回目标档位");

        let restored = guard.restore_now().expect("回滚失败");
        println!("  回滚 {restored} 个寄存器");
    }

    let final_fields = ctl.raw_fields().expect("最终读取失败");
    println!(
        "最终档位字段: 751=0x{:02X}, 7AB=0x{:02X}",
        final_fields.0, final_fields.1
    );
    assert_eq!(final_fields, (f1, f2), "档位寄存器必须完全恢复");
}
