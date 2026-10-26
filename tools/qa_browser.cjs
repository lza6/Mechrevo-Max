/* v0.5.x 前端真实浏览器路径验证（独立 Chromium + mock IPC，不访问硬件） */
const path = require('node:path');
const os = require('node:os');

function loadPlaywright() {
  const candidates = [
    process.env.PLAYWRIGHT_MODULE,
    'playwright-core',
    path.join(os.homedir(), 'AppData/Roaming/npm/node_modules/gsd-pi/node_modules/playwright-core'),
  ].filter(Boolean);
  for (const candidate of candidates) {
    try {
      return require(candidate);
    } catch (error) {
      if (error.code !== 'MODULE_NOT_FOUND') throw error;
    }
  }
  throw new Error('找不到 Playwright。请设置 PLAYWRIGHT_MODULE 指向 playwright-core 的安装目录。');
}

const { chromium } = loadPlaywright();

const root = path.resolve(__dirname, '..');
const url = 'file:///' + path.join(root, 'app/ui/index.html').replace(/\\/g, '/');
const errors = [];
const results = [];
const ok = (name, pass, detail = '') => results.push({ name, pass: !!pass, detail: String(detail) });

const ready = (overrides = {}) => ({
  ec: { kind: 'ready' }, project_id: 16, power_source: 7, is_ac_powered: true,
  cpu_temp_c: 56, gpu_temp_c: 42, cpu_fan_rpm: 3600, gpu_fan_rpm: 2200,
  mode: 'balance', mode_raw: [0x40, 0],
  gaming_power: { pl1: 75, pl2: 75, pl4: 200 },
  office_power: { pl1: 45, pl2: 45, pl4: 200 },
  fan_boost: true, firmware_power: { pl1: 75, pl2: 130, pl4: 200, extra: 133 },
  can_undo_power: false, can_undo_mode: false, can_undo_fan_boost: false, last_error: null,
  read_warnings: [],
  app_version: '0.5.0-test', log_dir: 'C:\\Users\\test\\AppData\\Local\\Mechrevo-Max\\logs',
  limits_meta: { pl1: [5, 200], pl2: [5, 200], pl4: [0, 255] },
  system_slow_stale: false,
  system: {
    cpu_util_pct: 18, mem_used_mb: 22000, mem_total_mb: 40000, mem_used_pct: 55,
    gpu: { name: 'RTX 4060', util_pct: 6, mem_util_pct: 22, temp_c: 42, clock_mhz: 210, mem_clock_mhz: 405, power_w: 5.9, vram_used_mb: 1288, vram_total_mb: 8188 },
    battery: { on_ac: true, charging: false, discharging: false, percent: 100, remaining_mwh: 46740, full_mwh: 46740, design_mwh: 0, voltage_mv: 11986, cycles: null, temp_c: null, charge_limit_pct: 80 },
    disk: { model: 'YMTC PC300', size_gb: 953, health: 'Healthy', temp_c: 41, temp_max_c: 83, wear_pct: 0 },
    device: { cpu: 'i9-12900HX', cpu_threads: 24, cpu_cores: 16, mem_total_mb: 40000, os: 'Windows 10 Pro' },
  },
  ...overrides,
});

const curveData = {
  blocks: [
    { base: 0x0f00, kind: 'temperature_points', values: [54, 59, 63, 66] },
    { base: 0x0f10, kind: 'temperature_points', values: [48, 50, 64, 67] },
    { base: 0x0f20, kind: 'duty_percent', values: [192, 192, 196, 196] },
    { base: 0x0f30, kind: 'temperature_points', values: [56, 58, 61, 64] },
    { base: 0x0f40, kind: 'temperature_points', values: [48, 48, 59, 62] },
    { base: 0x0f50, kind: 'duty_percent', values: [192, 196, 196, 196] },
  ],
  curves: [
    { label: '曲线 A（候选）', points: [{ temp_c: 54, duty_pct: 75 }, { temp_c: 59, duty_pct: 76 }, { temp_c: 63, duty_pct: 77 }] },
    { label: '曲线 B（候选）', points: [{ temp_c: 56, duty_pct: 75 }, { temp_c: 58, duty_pct: 76 }, { temp_c: 61, duty_pct: 77 }] },
  ],
};

(async () => {
  const browser = await chromium.launch({ headless: true, channel: 'chrome' });
  console.log('[qa] browser launched');
  const context = await browser.newContext({ viewport: { width: 960, height: 653 } });
  const page = await context.newPage();
  const calls = [];
  let status = ready();
  let curveCalls = 0;
  let curveFails = false;
  page.on('pageerror', e => errors.push('pageerror: ' + e.message));
  page.on('console', m => { if (m.type() === 'error') errors.push('console.error: ' + m.text()); });
  // 确认框按用例单独 accept/dismiss；不能全局自动接受，否则测不到取消分支。
  await page.addInitScript((payload) => {
    window.__ipcCalls = [];
    window.__curveCalls = 0;
    window.__curveData = payload.curveData;
    window.__status = payload.status;
    window.__curveFails = false;
    if (window.__profilesBroken === undefined) window.__profilesBroken = !!payload.brokenProfiles;
    window.__TAURI__ = { core: { invoke: async (cmd, args) => {
      window.__ipcCalls.push({ cmd, args });
      if (cmd === 'get_status') return window.__status;
      if (cmd === 'get_fan_curves') {
        window.__curveCalls += 1;
        if (window.__curveFails) throw new Error('EC 驱动未就绪');
        return window.__curveData;
      }
      if (cmd === 'list_profiles') {
        if (window.__profilesBroken) return { kind: 'needs_attention', message: '配置含非法条目，已保留原文件', backup: 'C:\\Temp\\profiles.json.broken' };
        return { kind: 'ready', profiles: [] };
      }
      if (cmd === 'restore_profiles') {
        if (!window.__profilesBroken) throw new Error('当前没有损坏配置可恢复');
        window.__profilesBroken = false;
        return [{ name: 'restored', pl1: 60, pl2: 45, pl4: 200, fan_boost: false }];
      }
      return 'ok';
    } } };
  }, { curveData, status, brokenProfiles: false });
  console.log('[qa] init injected');
  await page.goto(url + '?audit=' + Date.now(), { waitUntil: 'load' });
  console.log('[qa] page loaded');
  await page.waitForFunction(() => document.querySelector('#ec-text')?.textContent === 'EC 已连接', { timeout: 10000 });
  console.log('[qa] ready');

  ok('启动默认概览', await page.locator('.page[data-page="overview"][data-active]').count() === 1);
  ok('8 个仪表渲染', await page.locator('.gauge').count() === 8);
  ok('版本已显示', (await page.locator('#dev-version').textContent()) === '0.5.0-test');
  ok('日志目录已显示', (await page.locator('#dev-log').textContent()).includes('Mechrevo-Max'));
  ok('曲线自动加载', await page.evaluate(() => window.__curveCalls) === 1);

  await page.locator('.nav-item[data-page="cooling"]').click();
  await page.waitForFunction(() => document.querySelectorAll('.curve-card svg').length === 2);
  ok('曲线两张图与坐标轴', await page.locator('.curve-card svg').count() === 2 && (await page.locator('.curve-svg').first().innerHTML()).includes('°C'));
  await page.screenshot({ path: path.join(root, 'docs/screenshots/audit-cooling.png') });

  // EC 不可用：免驱 CPU/内存仍可读，不应整页清空
  status = ready({ ec: { kind: 'unavailable', message: '驱动未就绪', hint: 'driver' } });
  await page.evaluate((s) => { window.__status = s; }, status);
  await page.waitForTimeout(2300);
  ok('EC 不可用时 CPU 仪表仍有值', /\d/.test(await page.locator('#g-cpu-util .value').textContent()));
  ok('EC 不可用时免驱内存仍有值', /\d/.test(await page.locator('#g-mem .value').textContent()));
  ok('EC 不可用时 EC 温度降级', (await page.locator('#g-cpu-temp .value').textContent()).includes('—'));

  // 无电池 + 无 GPU + 未知内存：全部未知，不显示 0
  status = ready({ system: { cpu_util_pct: 10, mem_used_mb: null, mem_total_mb: null, mem_used_pct: null, gpu: null, battery: null, disk: null, device: { cpu: 'i9', cpu_threads: 24, cpu_cores: 16, os: 'Windows' } } });
  await page.evaluate((s) => { window.__status = s; }, status);
  await page.locator('.nav-item[data-page="battery"]').click();
  await page.waitForTimeout(2300);
  ok('无电池状态明确', (await page.locator('#batt-state').textContent()) === '无电池数据');
  ok('无电池电量显示未知而非 0', (await page.locator('#batt-pct').textContent()).trim() === '—');
  ok('未知 GPU 显示未知', (await page.locator('#g-gpu-util .value').textContent()).includes('—'));
  ok('未知内存显示未知', (await page.locator('#g-mem .value').textContent()).includes('—'));

  // 曲线失败：可行动错误 + 30 秒退避，不因轮询刷掉
  await page.evaluate(() => { window.__curveFails = true; window.__curveCalls = 0; });
  await page.locator('.nav-item[data-page="cooling"]').click();
  await page.locator('#reload-curves').click();
  await page.waitForFunction(() => document.querySelector('#curve-meta').textContent.includes('30 秒'));
  ok('曲线失败明确提示重试', (await page.locator('#curve-meta').textContent()).includes('30 秒后可重试'));
  await page.locator('.nav-item[data-page="overview"]').click();
  await page.locator('.nav-item[data-page="cooling"]').click();
  ok('曲线失败退避不重复调用', await page.evaluate(() => window.__curveCalls) === 1);
  ok('失败可恢复按钮可用', await page.locator('#reload-curves').isEnabled());
  await page.screenshot({ path: path.join(root, 'docs/screenshots/audit-cooling-error.png') });

  // 页面切换不丢失错误提示
  await page.locator('.nav-item[data-page="advanced"]').click();
  ok('导航到高级页', await page.locator('.page[data-page="advanced"][data-active]').count() === 1);
  await page.screenshot({ path: path.join(root, 'docs/screenshots/audit-advanced.png') });

  // 危险写入确认：拒绝不发 IPC，接受才发 IPC。
  await page.locator('.nav-item[data-page="performance"]').click();
  const sliderRange = await page.locator('#pl1').evaluate(e => [e.min, e.max]);
  ok('滑块范围来自后端', sliderRange[0] === '5' && sliderRange[1] === '200', sliderRange.join('/'));
  await page.evaluate(() => { window.__ipcCalls = []; });
  page.once('dialog', dialog => dialog.dismiss());
  await page.locator('.mode-btn[data-mode="turbo"]').click();
  await page.waitForTimeout(200);
  ok('取消切档不会发 IPC', await page.evaluate(() => window.__ipcCalls.filter(c => c.cmd === 'set_mode').length) === 0);
  page.once('dialog', dialog => dialog.accept());
  await page.locator('.mode-btn[data-mode="turbo"]').click();
  await page.waitForFunction(() => window.__ipcCalls.some(c => c.cmd === 'set_mode'));
  ok('确认切档才发 IPC', await page.evaluate(() => window.__ipcCalls.some(c => c.cmd === 'set_mode' && c.args.mode === 'turbo')));

  // 查询失败/超期必须标注缓存，不得冒充实时值。
  await page.evaluate(() => {
    const s = window.__status;
    s.system_slow_stale = true;
    window.__status = s;
  });
  await page.locator('.nav-item[data-page="monitor"]').click();
  await page.waitForTimeout(2300);
  ok('慢速查询失败显示缓存提示', (await page.locator('#monitor-meta').textContent()).includes('缓存'));

  // 部分 EC 读取失败：其他字段保留，警告必须可见，下一轮正常后清除。
  await page.evaluate(() => {
    const s = window.__status;
    s.read_warnings = ['游戏默认功耗读取失败: mock EC error'];
    window.__status = s;
  });
  await page.waitForTimeout(2300);
  ok('部分 EC 失败常驻提示', (await page.locator('#persist-error').textContent()).includes('部分 EC 数据读取失败'));
  await page.evaluate(() => { window.__status.read_warnings = []; });
  await page.waitForTimeout(2300);
  ok('部分失败下一轮正常后清除', await page.locator('#persist-error').isHidden());

  // 配置损坏：不能显示为“没有配置”，必须明确需要处理。
  await page.addInitScript(() => { window.__profilesBroken = true; });
  await page.locator('.nav-item[data-page="performance"]').click();
  await page.reload({ waitUntil: 'load' });
  await page.waitForFunction(() => document.querySelector('#ec-text')?.textContent === 'EC 已连接');
  await page.waitForFunction(() => document.querySelector('#status-msg')?.textContent.includes('配置含非法条目'));
  ok('配置损坏明确提示', (await page.locator('#status-msg').textContent()).includes('配置含非法条目'));
  ok('配置损坏不伪报空列表', !(await page.locator('#profile-list').textContent()).includes('还没有自定义配置'));

  // 恢复入口：确认后才调用恢复命令，不能自动覆盖。
  ok('损坏配置提供恢复入口', await page.locator('#profile-list button').filter({ hasText: '从备份恢复' }).count() === 1);
  page.once('dialog', dialog => dialog.accept());
  await page.locator('#profile-list button').filter({ hasText: '从备份恢复' }).click();
  await page.waitForFunction(() => document.querySelector('#status-msg')?.textContent.includes('已从备份恢复配置'));
  ok('确认后从备份恢复', await page.evaluate(() => window.__ipcCalls.some(c => c.cmd === 'restore_profiles')));
  ok('恢复后显示恢复的配置', (await page.locator('#profile-list').textContent()).includes('restored'));

  // ---- 风扇曲线写入（T-040-04 F-9）：输入校验 + 取消不发 IPC + 确认才发 ----
  await page.locator('.nav-item[data-page="cooling"]').click();
  await page.evaluate(() => { window.__ipcCalls = []; });
  // 非法输入：占空比低于安全下界
  await page.locator('#curve-temps').fill('50, 60');
  await page.locator('#curve-duties').fill('192, 10');
  await page.locator('#apply-curve').click();
  ok('曲线写入拒绝低于安全下界的占空比', (await page.locator('#curve-write-status').textContent()).includes('安全下界'));
  ok('非法输入不发 IPC', await page.evaluate(() => window.__ipcCalls.filter(c => c.cmd === 'set_fan_curve').length) === 0);
  // 非单调温度
  await page.locator('#curve-temps').fill('60, 50');
  await page.locator('#curve-duties').fill('192, 192');
  await page.locator('#apply-curve').click();
  ok('曲线写入拒绝非单调温度', (await page.locator('#curve-write-status').textContent()).includes('单调'));
  // 合法输入 → 取消
  await page.locator('#curve-temps').fill('50, 60');
  await page.locator('#curve-duties').fill('192, 196');
  page.once('dialog', dialog => dialog.dismiss());
  await page.locator('#apply-curve').click();
  await page.waitForTimeout(200);
  ok('取消曲线写入不发 IPC', await page.evaluate(() => window.__ipcCalls.filter(c => c.cmd === 'set_fan_curve').length) === 0);
  // 合法输入 → 确认
  page.once('dialog', dialog => dialog.accept());
  await page.locator('#apply-curve').click();
  await page.waitForFunction(() => window.__ipcCalls.some(c => c.cmd === 'set_fan_curve'), { timeout: 5000 });
  ok('确认曲线写入才发 IPC 且参数正确', await page.evaluate(() => {
    const c = window.__ipcCalls.find(x => x.cmd === 'set_fan_curve');
    return c && c.args.tempBlock === 0x0F00 && c.args.dutyBlock === 0x0F20 &&
      JSON.stringify(c.args.temps) === '[50,60]' && JSON.stringify(c.args.duties) === '[192,196]';
  }));
  await page.screenshot({ path: path.join(root, 'docs/screenshots/audit-curve-write.png') });

  const failed = results.filter(r => !r.pass);
  console.log(JSON.stringify({ total: results.length, passed: results.filter(r => r.pass).length, failed, errors, results }, null, 2));
  await browser.close();
  process.exitCode = failed.length || errors.length ? 1 : 0;
})().catch(async (e) => {
  console.error(e.stack || e);
  process.exitCode = 1;
});
