// 机械革命控制台 · 前端逻辑
// 与 Rust 后端通过 Tauri IPC 通信

const { invoke } = window.__TAURI__.core;

/**
 * IPC 契约表 —— 后端命令名的唯一来源（ipc_contract.rs 会解析本表做双向比对）。
 * 约定：任何 invoke 都必须经由本表。
 */
const IPC = {
  getStatus: 'get_status',
  setMode: 'set_mode',
  setPowerLimits: 'set_power_limits',
  undoPower: 'undo_power',
  undoMode: 'undo_mode',
  setFanBoost: 'set_fan_boost',
  undoFanBoost: 'undo_fan_boost',
  readRegisters: 'read_registers',
  listProfiles: 'list_profiles',
  restoreProfiles: 'restore_profiles',
  saveProfile: 'save_profile',
  deleteProfile: 'delete_profile',
  applyProfile: 'apply_profile',
  getFanCurves: 'get_fan_curves',
  setFanCurve: 'set_fan_curve',
  relaunchElevated: 'relaunch_elevated',
};

const POLL_MS = 2000;
const TEMP_WARM = 60;
const TEMP_HOT = 80;
const MAX_EC_ADDR = 0x0fff;
const MAX_REG_QUERY = 64;
const CURVE_REFRESH_MS = 60000;
const CURVE_RETRY_MS = 30000;

const el = (id) => document.getElementById(id);

function findMissingElements(map) {
  return Object.entries(map).filter(([, node]) => !node).map(([name]) => name);
}

const ui = {
  nav: el('nav'),
  ecPill: el('ec-pill'),
  ecText: el('ec-text'),
  projectId: el('project-id'),
  sampleInfo: el('sample-info'),
  alert: el('ec-alert'),
  alertTitle: el('ec-alert-title'),
  alertMsg: el('ec-alert-msg'),
  alertDriver: el('alert-driver'),
  relaunch: el('relaunch'),
  gCpuUtil: el('g-cpu-util'),
  gCpuTemp: el('g-cpu-temp'),
  gGpuUtil: el('g-gpu-util'),
  gGpuTemp: el('g-gpu-temp'),
  gGpuPower: el('g-gpu-power'),
  gMem: el('g-mem'),
  gBattery: el('g-battery'),
  gDiskTemp: el('g-disk-temp'),
  gCpuName: el('g-cpu-name'),
  gGpuName: el('g-gpu-name'),
  chipMode: el('chip-mode'),
  chipBoost: el('chip-boost'),
  chipAc: el('chip-ac'),
  chipFan: el('chip-fan'),
  sensorTable: el('sensor-table'),
  monitorMeta: el('monitor-meta'),
  statsHint: el('stats-hint'),
  resetStats: el('reset-stats'),
  devCpu: el('dev-cpu'),
  devCpuCores: el('dev-cpu-cores'),
  devCpuUtil: el('dev-cpu-util'),
  devCpuTemp: el('dev-cpu-temp'),
  devCpuFan: el('dev-cpu-fan'),
  devGpu: el('dev-gpu'),
  devGpuUtil: el('dev-gpu-util'),
  devGpuTemp: el('dev-gpu-temp'),
  devGpuClock: el('dev-gpu-clock'),
  devGpuVram: el('dev-gpu-vram'),
  devMemTotal: el('dev-mem-total'),
  devMemUsed: el('dev-mem-used'),
  devDisk: el('dev-disk'),
  devDiskCap: el('dev-disk-cap'),
  devDiskTemp: el('dev-disk-temp'),
  devDiskWear: el('dev-disk-wear'),
  devOs: el('dev-os'),
  devProject: el('dev-project'),
  devVersion: el('dev-version'),
  devLog: el('dev-log'),
  devPoll: el('dev-poll'),
  battState: el('batt-state'),
  battFill: el('batt-fill'),
  battPct: el('batt-pct'),
  battRemain: el('batt-remain'),
  battFull: el('batt-full'),
  battVolt: el('batt-volt'),
  battLimit: el('batt-limit'),
  battDesign: el('batt-design'),
  battWear: el('batt-wear'),
  battCycles: el('batt-cycles'),
  battTemp: el('batt-temp'),
  modeList: el('mode-list'),
  modeRaw: el('mode-raw'),
  bfMode: el('bf-mode'),
  bfFanBoost: el('bf-fanboost'),
  fanBoost: el('fan-boost'),
  undoMode: el('undo-mode'),
  undoFan: el('undo-fan'),
  powerTabs: el('power-tabs'),
  pl1: el('pl1'),
  pl2: el('pl2'),
  pl4: el('pl4'),
  pl1Out: el('pl1-out'),
  pl2Out: el('pl2-out'),
  pl4Out: el('pl4-out'),
  pending: el('pending'),
  applyPower: el('apply-power'),
  undoPower: el('undo-power'),
  resetPower: el('reset-power'),
  fwPl: el('fw-pl'),
  profileName: el('profile-name'),
  saveProfile: el('save-profile'),
  profileList: el('profile-list'),
  profilePath: el('profile-path'),
  curveBody: el('curve-body'),
  curveMeta: el('curve-meta'),
  reloadCurves: el('reload-curves'),
  curveTemps: el('curve-temps'),
  curveDuties: el('curve-duties'),
  applyCurve: el('apply-curve'),
  curveWriteStatus: el('curve-write-status'),
  regAddrs: el('reg-addrs'),
  readRegs: el('read-regs'),
  exportRegs: el('export-regs'),
  regOut: el('reg-out'),
  statusMsg: el('status-msg'),
  persistError: el('persist-error'),
  alertLive: el('alert-live'),
  logPath: el('log-path'),
  clock: el('clock'),
};

const stats = { samples: 0, lastAt: null, lastGapMs: null, byKey: {} };
let lastStatus = null;
let lastRegRead = null;
let curvesLoadedAt = 0;
let curveRetryAfter = 0;
let limitsSynced = false;
let activePowerMode = 'gaming';
let baseline = { gaming: null, office: null };
let ecReady = false;
let pollTimer = null;
let refreshing = false;
let pendingRefresh = false;
let editingPower = false;

// ---------- 基础工具 ----------

function setStatus(msg, kind = '') {
  ui.statusMsg.textContent = msg;
  ui.statusMsg.dataset.kind = kind;
}

function setBusy(busy) {
  document.body.dataset.busy = busy ? 'true' : 'false';
  if (busy) stopPolling();
  else startPolling();
}

function startPolling() {
  if (pollTimer !== null) return;
  pollTimer = setInterval(refresh, POLL_MS);
}

function stopPolling() {
  if (pollTimer === null) return;
  clearInterval(pollTimer);
  pollTimer = null;
}

const fmt = (v, unit = '', fallback = '—') => (v === null || v === undefined ? fallback : `${v}${unit}`);
const pct = (v) => (v === null || v === undefined ? '—' : `${Math.round(v)} %`);
const onOff = (v) => (v === null || v === undefined ? '—' : v ? '开' : '关');

function paintRange(input) {
  const min = Number(input.min);
  const max = Number(input.max);
  const val = Number(input.value);
  const fill = max > min ? ((val - min) / (max - min)) * 100 : 0;
  input.style.setProperty('--fill', `${fill}%`);
}

// ---------- 环形仪表 ----------

const GAUGE_R = 46;
const GAUGE_C = 2 * Math.PI * GAUGE_R;

function gaugeHtml(spec) {
  const { value, unit, label, max = 100, warm = null, hot = null } = spec;
  const has = value !== null && value !== undefined;
  const ratio = has ? Math.max(0, Math.min(1, value / max)) : 0;
  const offset = GAUGE_C * (1 - ratio);
  const level = !has ? 'none' : hot !== null && value >= hot ? 'hot' : warm !== null && value >= warm ? 'warm' : 'ok';
  const shown = has ? Math.round(value) : '—';
  return '<div class="gauge" data-level="' + level + '">' +
    '<svg viewBox="0 0 108 108" role="img" aria-label="' + label + ' ' + (has ? shown + unit : '无数据') + '">' +
    '<circle class="track" cx="54" cy="54" r="' + GAUGE_R + '"></circle>' +
    '<circle class="arc" cx="54" cy="54" r="' + GAUGE_R + '" stroke-dasharray="' + GAUGE_C.toFixed(1) +
    '" stroke-dashoffset="' + offset.toFixed(1) + '"></circle></svg>' +
    '<div class="value"><b>' + shown + '</b><span>' + unit + '</span></div></div>' +
    '<p class="gauge-caption">' + label + '</p>';
}

function putGauge(node, spec) {
  if (node) node.innerHTML = gaugeHtml(spec);
}

function renderOverview(s) {
  const sys = s.system || {};
  const gpu = sys.gpu || null;
  const batt = sys.battery || null;
  const disk = sys.disk || null;
  const ready = s.ec.kind === 'ready';

  putGauge(ui.gCpuUtil, { value: sys.cpu_util_pct, unit: '%', label: 'CPU 占用', max: 100, warm: 80, hot: 95 });
  putGauge(ui.gCpuTemp, { value: ready ? s.cpu_temp_c : null, unit: '°C', label: 'CPU 温度', max: 100, warm: TEMP_WARM, hot: TEMP_HOT });
  putGauge(ui.gGpuUtil, { value: gpu ? gpu.util_pct : null, unit: '%', label: 'GPU 占用', max: 100, warm: 85, hot: 97 });
  putGauge(ui.gGpuTemp, { value: gpu ? gpu.temp_c : null, unit: '°C', label: 'GPU 温度', max: 100, warm: 70, hot: 85 });
  putGauge(ui.gGpuPower, { value: gpu ? gpu.power_w : null, unit: 'W', label: 'GPU 功耗', max: 150 });
  putGauge(ui.gMem, { value: sys.mem_used_pct, unit: '%', label: '内存占用', max: 100, warm: 85, hot: 95 });
  putGauge(ui.gBattery, { value: batt && batt.percent !== undefined ? batt.percent : null, unit: '%', label: '电池', max: 100 });
  putGauge(ui.gDiskTemp, { value: disk && disk.temp_c !== null && disk.temp_c !== undefined ? disk.temp_c : null, unit: '°C', label: 'SSD 温度', max: 90, warm: 60, hot: 75 });

  ui.gCpuName.textContent = sys.device && sys.device.cpu ? sys.device.cpu : '—';
  ui.gGpuName.textContent = gpu ? gpu.name : '—';

  const label = { balance: '均衡', office: '办公', turbo: '狂暴' }[s.mode];
  ui.chipMode.textContent = '档位 ' + (label || (s.mode ? s.mode : '未知编码'));
  ui.chipBoost.textContent = '强冷 ' + onOff(s.fan_boost);
  ui.chipBoost.dataset.on = String(!!s.fan_boost);
  ui.chipAc.textContent = '供电 ' + (s.is_ac_powered ? '适配器' : s.is_ac_powered === false ? '电池' : '—');
  ui.chipFan.textContent = ready ? '风扇 ' + s.cpu_fan_rpm + ' / ' + s.gpu_fan_rpm + ' RPM' : '风扇 —';
}

// ---------- 采样统计 ----------

function recordSample(key, value) {
  if (value === null || value === undefined || Number.isNaN(value)) return;
  const slot = stats.byKey[key] || { min: value, max: value, sum: 0, n: 0 };
  slot.min = Math.min(slot.min, value);
  slot.max = Math.max(slot.max, value);
  slot.sum += value;
  slot.n += 1;
  stats.byKey[key] = slot;
}

function statOf(key, which) {
  const slot = stats.byKey[key];
  if (!slot || slot.n === 0) return null;
  if (which === 'min') return slot.min;
  if (which === 'max') return slot.max;
  return slot.sum / slot.n;
}

function resetStats() {
  stats.samples = 0;
  stats.byKey = {};
  stats.lastAt = null;
  stats.lastGapMs = null;
  renderSampleInfo();
  renderMonitor(lastStatus);
}

function renderSampleInfo() {
  const gap = stats.lastGapMs === null ? '—' : (stats.lastGapMs / 1000).toFixed(1) + 's';
  const since = stats.lastAt === null ? '—' : Math.round((Date.now() - stats.lastAt) / 1000) + 's 前';
  ui.sampleInfo.textContent = (POLL_MS / 1000).toFixed(1) + 's · ' + gap + ' · ' + stats.samples + ' 次';
  ui.sampleInfo.title = '采样间隔 ' + POLL_MS / 1000 + 's；上次刷新 ' + since;
  ui.statsHint.textContent = '本次启动已采样 ' + stats.samples + ' 次 · 上次 ' + since;
}

// ---------- EC 状态 ----------

function renderEc(ec) {
  if (ec.kind === 'ready') {
    ecReady = true;
    ui.ecPill.dataset.state = 'ready';
    ui.ecText.textContent = 'EC 已连接';
    ui.alert.hidden = true;
    ui.alertLive.textContent = '';
    return;
  }
  ecReady = false;
  ui.ecPill.dataset.state = 'error';
  ui.ecText.textContent = 'EC 未连接';
  ui.alertMsg.textContent = ec.message;
  ui.alert.hidden = false;

  const needsElevation = ec.hint === 'elevation';
  const needsRestart = ec.hint === 'restart';
  ui.relaunch.hidden = !(needsElevation || needsRestart);
  ui.alertDriver.hidden = needsElevation || needsRestart;
  ui.alertTitle.textContent = needsElevation ? '需要管理员权限' : needsRestart ? '需要重启程序' : 'EC 通道不可用';
  ui.alertLive.textContent = ui.alertTitle.textContent + '：' + ec.message;
}

// ---------- 监控表 ----------

const SENSOR_UNITS = {
  cpu_util: ' %', cpu_temp: ' °C', gpu_util: ' %', gpu_temp: ' °C', gpu_power: ' W',
  mem: ' %', cpu_fan: ' RPM', gpu_fan: ' RPM', battery: ' %', power_src: '',
};

function sensorLevel(key, value) {
  if (value === null || value === undefined) return 'none';
  if (key.slice(-4) === 'temp') return value >= TEMP_HOT ? 'hot' : value >= TEMP_WARM ? 'warm' : 'ok';
  if (key === 'cpu_util' || key === 'gpu_util' || key === 'mem') return value >= 95 ? 'hot' : value >= 85 ? 'warm' : 'ok';
  return 'ok';
}

function sensorState(key, value) {
  if (value === null || value === undefined) return '无数据';
  if (key.slice(-4) === 'temp') return value >= TEMP_HOT ? '过热' : value >= TEMP_WARM ? '偏高' : '正常';
  if (key === 'cpu_util' || key === 'gpu_util') return value >= 90 ? '满载' : value >= 50 ? '繁忙' : '轻载';
  if (key === 'mem') return value >= 90 ? '紧张' : '正常';
  if (key === 'cpu_fan' || key === 'gpu_fan') return value === 0 ? '静止' : value >= 4500 ? '高转速' : '运转中';
  return '—';
}

function renderMonitor(s) {
  if (!s) return;
  const sys = s.system || {};
  const gpu = sys.gpu || null;
  const batt = sys.battery || null;
  const ready = s.ec.kind === 'ready';

  const rows = {
    cpu_util: sys.cpu_util_pct === undefined ? null : sys.cpu_util_pct,
    cpu_temp: ready ? s.cpu_temp_c : null,
    gpu_util: gpu ? gpu.util_pct : null,
    gpu_temp: gpu ? gpu.temp_c : null,
    gpu_power: gpu ? gpu.power_w : null,
    mem: sys.mem_used_pct === undefined ? null : sys.mem_used_pct,
    cpu_fan: ready ? s.cpu_fan_rpm : null,
    gpu_fan: ready ? s.gpu_fan_rpm : null,
    battery: batt && batt.percent !== undefined ? batt.percent : null,
    power_src: ready ? (s.is_ac_powered ? '适配器' : '电池') : null,
  };

  for (const key of Object.keys(rows)) {
    const value = rows[key];
    const tr = ui.sensorTable.querySelector('tr[data-sensor="' + key + '"]');
    if (!tr) continue;
    const unit = SENSOR_UNITS[key] === undefined ? '' : SENSOR_UNITS[key];
    const numeric = typeof value === 'number';
    tr.querySelector('[data-col="cur"]').textContent = fmt(value, unit);
    for (const col of ['min', 'avg', 'max']) {
      const v = numeric ? statOf(key, col) : null;
      tr.querySelector('[data-col="' + col + '"]').textContent = v === null ? '—' : Math.round(v) + unit;
    }
    tr.querySelector('[data-col="state"]').textContent = sensorState(key, value);
    tr.dataset.level = sensorLevel(key, value);
  }

  ui.monitorMeta.textContent = (ready ? '采样 ' + stats.samples + ' 次' : 'EC 不可用（部分数据仍可读）')
    + (s.system_slow_stale ? ' · 电池/SSD 为缓存（查询失败或超期）' : '');
}

// ---------- 设备信息 ----------

function renderDevice(s) {
  const sys = s.system || {};
  const dev = sys.device || {};
  const gpu = sys.gpu || null;
  const disk = sys.disk || null;
  const ready = s.ec.kind === 'ready';

  ui.devCpu.textContent = dev.cpu || '—';
  ui.devCpuCores.textContent = dev.cpu_cores ? dev.cpu_cores + ' 核 / ' + dev.cpu_threads + ' 线程' : (dev.cpu_threads || '—') + ' 线程';
  ui.devCpuUtil.textContent = pct(sys.cpu_util_pct);
  ui.devCpuTemp.textContent = fmt(ready ? s.cpu_temp_c : null, ' °C');
  ui.devCpuFan.textContent = fmt(ready ? s.cpu_fan_rpm : null, ' RPM');

  ui.devGpu.textContent = gpu ? gpu.name : '—（无 nvidia-smi）';
  ui.devGpuUtil.textContent = gpu ? gpu.util_pct + ' % / ' + gpu.mem_util_pct + ' %' : '—';
  ui.devGpuTemp.textContent = gpu ? gpu.temp_c + ' °C / ' + gpu.power_w + ' W' : '—';
  ui.devGpuClock.textContent = gpu ? gpu.clock_mhz + ' MHz / ' + gpu.mem_clock_mhz + ' MHz' : '—';
  ui.devGpuVram.textContent = gpu ? gpu.vram_used_mb + ' / ' + gpu.vram_total_mb + ' MiB' : '—';

  ui.devMemTotal.textContent = typeof sys.mem_total_mb === 'number' ? (sys.mem_total_mb / 1024).toFixed(1) + ' GB' : '—';
  ui.devMemUsed.textContent = typeof sys.mem_total_mb === 'number' && typeof sys.mem_used_mb === 'number'
    ? (sys.mem_used_mb / 1024).toFixed(1) + ' GB / ' + pct(sys.mem_used_pct)
    : '—';

  ui.devDisk.textContent = disk && disk.model ? disk.model : '—';
  ui.devDiskCap.textContent = disk ? disk.size_gb + ' GB / ' + (disk.health || '—') : '—';
  ui.devDiskTemp.textContent = disk && disk.temp_c !== null && disk.temp_c !== undefined
    ? disk.temp_c + ' °C / 上限 ' + (disk.temp_max_c === null || disk.temp_max_c === undefined ? '—' : disk.temp_max_c) + ' °C'
    : '—';
  ui.devDiskWear.textContent = disk && disk.wear_pct !== null && disk.wear_pct !== undefined ? disk.wear_pct + ' %' : '—';

  ui.devOs.textContent = dev.os || '—';
  ui.devProject.textContent = fmt(s.project_id);
  ui.devPoll.textContent = (POLL_MS / 1000).toFixed(1) + ' 秒';
  ui.devVersion.textContent = s.app_version || '—';
  ui.devLog.textContent = s.log_dir || '—';
  ui.devLog.title = s.log_dir || '';
  ui.logPath.textContent = s.log_dir || '';
  ui.logPath.title = s.log_dir || '';
}

// ---------- 电池 ----------

function renderBattery(s) {
  const batt = (s.system || {}).battery || null;
  const na = [ui.battRemain, ui.battFull, ui.battVolt, ui.battLimit, ui.battDesign, ui.battWear, ui.battCycles, ui.battTemp];
  if (!batt) {
    ui.battState.textContent = '无电池数据';
    ui.battPct.textContent = '—';
    ui.battFill.style.width = '0%';
    for (const n of na) n.textContent = '—';
    return;
  }

  ui.battState.textContent = batt.charging ? '充电中' : batt.discharging ? '放电中' : batt.on_ac ? '接电 · 未充电' : '电池供电';
  ui.battPct.textContent = typeof batt.percent === 'number' ? batt.percent + ' %' : '—';
  ui.battFill.style.width = typeof batt.percent === 'number' ? Math.max(0, Math.min(100, batt.percent)) + '%' : '0%';

  ui.battRemain.textContent = batt.remaining_mwh ? (batt.remaining_mwh / 1000).toFixed(2) + ' Wh' : '—';
  ui.battFull.textContent = batt.full_mwh ? (batt.full_mwh / 1000).toFixed(2) + ' Wh' : '—';
  ui.battVolt.textContent = batt.voltage_mv ? (batt.voltage_mv / 1000).toFixed(3) + ' V' : '—';
  ui.battLimit.textContent = batt.charge_limit_pct === null || batt.charge_limit_pct === undefined
    ? '本机不提供' : batt.charge_limit_pct + ' %';

  ui.battDesign.textContent = batt.design_mwh ? (batt.design_mwh / 1000).toFixed(2) + ' Wh' : '本机不提供';
  ui.battWear.textContent = batt.design_mwh && batt.full_mwh
    ? (100 - (batt.full_mwh * 100) / batt.design_mwh).toFixed(1) + ' %'
    : '需设计容量（本机不提供）';
  ui.battCycles.textContent = batt.cycles === null || batt.cycles === undefined ? '本机不提供' : batt.cycles + ' 次';
  ui.battTemp.textContent = batt.temp_c === null || batt.temp_c === undefined ? '本机不提供' : batt.temp_c + ' °C';
}

// ---------- 性能 ----------

function renderMode(mode) {
  for (const btn of ui.modeList.querySelectorAll('.mode-btn')) {
    btn.setAttribute('aria-pressed', btn.dataset.mode === mode ? 'true' : 'false');
    btn.disabled = !ecReady;
  }
}

const hex2 = (v) => '0x' + v.toString(16).toUpperCase().padStart(2, '0');
const modeLabel = (key) => ({ balance: '均衡', office: '办公', turbo: '狂暴' }[key] || null);

function renderBitfield(raw, mode) {
  if (!raw) {
    ui.modeRaw.textContent = '751=— · 7AB=—';
    ui.bfMode.textContent = '—';
    ui.bfFanBoost.textContent = '—';
    return;
  }
  const a = raw[0];
  const b = raw[1];
  ui.modeRaw.textContent = '751=' + hex2(a) + ' · 7AB=' + hex2(b);
  const label = modeLabel(mode);
  ui.bfMode.textContent = hex2(a & 0xbf) + ' → ' + (label || '未知编码（不猜测）');
  ui.bfFanBoost.textContent = ((a & 0x40) !== 0 ? '开' : '关') + '（' + hex2(a & 0x40) + '）';
}

function readSliders() {
  return { pl1: Number(ui.pl1.value), pl2: Number(ui.pl2.value), pl4: Number(ui.pl4.value) };
}

const SLIDER_SEMANTICS = [
  { input: 'pl1', out: 'pl1Out', label: '长时功耗墙 PL1' },
  { input: 'pl2', out: 'pl2Out', label: '短时功耗墙 PL2' },
  { input: 'pl4', out: 'pl4Out', label: '峰值功耗墙 PL4' },
];

function syncSliderOutputs() {
  for (const item of SLIDER_SEMANTICS) {
    const node = ui[item.input];
    ui[item.out].textContent = node.value + ' W';
    node.setAttribute('aria-valuetext', node.value + ' 瓦，' + item.label);
    paintRange(node);
  }
  updateDirty();
}

function setSliders(limits) {
  if (!limits) return;
  ui.pl1.value = limits.pl1;
  ui.pl2.value = limits.pl2;
  ui.pl4.value = limits.pl4;
  syncSliderOutputs();
}

function updateDirty() {
  const base = baseline[activePowerMode];
  const cur = readSliders();
  const dirty = base ? cur.pl1 !== base.pl1 || cur.pl2 !== base.pl2 || cur.pl4 !== base.pl4 : false;
  ui.pending.textContent = dirty ? '有未应用的修改' : '未修改';
  ui.pending.dataset.dirty = dirty ? 'true' : 'false';
  ui.applyPower.disabled = !ecReady || !dirty;
}

function syncLimits(s) {
  if (limitsSynced || !s.limits_meta) return;
  for (const key of ['pl1', 'pl2', 'pl4']) {
    const range = s.limits_meta[key];
    if (!Array.isArray(range) || range.length !== 2) continue;
    ui[key].min = String(range[0]);
    ui[key].max = String(range[1]);
  }
  limitsSynced = true;
}

function renderPerformance(s) {
  syncLimits(s);
  renderMode(s.mode);
  renderBitfield(s.mode_raw, s.mode);

  if (s.fan_boost !== null && s.fan_boost !== undefined && document.activeElement !== ui.fanBoost) {
    ui.fanBoost.checked = !!s.fan_boost;
  }
  ui.fanBoost.disabled = !ecReady;

  ui.fwPl.textContent = s.firmware_power
    ? 'PL1 ' + s.firmware_power.pl1 + ' W · PL2 ' + s.firmware_power.pl2 + ' W · PL4 ' + s.firmware_power.pl4 + ' W'
    : '—';

  baseline.gaming = s.gaming_power;
  baseline.office = s.office_power;
  const dirty = ui.pending.dataset.dirty === 'true';
  if (!editingPower && !dirty) setSliders(baseline[activePowerMode]);
  updateDirty();

  ui.undoMode.disabled = !ecReady || !s.can_undo_mode;
  ui.undoPower.disabled = !ecReady || !s.can_undo_power;
  ui.undoFan.disabled = !ecReady || !s.can_undo_fan_boost;
}

// ---------- 主渲染 ----------

function renderStatus(s) {
  lastStatus = s;
  renderEc(s.ec);
  ui.projectId.textContent = fmt(s.project_id);

  const sys = s.system || {};
  if (s.ec.kind === 'ready') {
    recordSample('cpu_util', sys.cpu_util_pct);
    recordSample('cpu_temp', s.cpu_temp_c);
    if (sys.gpu) {
      recordSample('gpu_util', sys.gpu.util_pct);
      recordSample('gpu_temp', sys.gpu.temp_c);
      recordSample('gpu_power', sys.gpu.power_w);
    }
    recordSample('mem', sys.mem_used_pct);
    recordSample('cpu_fan', s.cpu_fan_rpm);
    recordSample('gpu_fan', s.gpu_fan_rpm);
  }
  if (sys.battery) recordSample('battery', sys.battery.percent);

  renderOverview(s);
  renderMonitor(s);
  renderDevice(s);
  renderBattery(s);
  renderPerformance(s);
  renderSampleInfo();

  ui.persistError.hidden = !s.last_error && !(s.read_warnings && s.read_warnings.length);
  const warnings = (s.read_warnings && s.read_warnings.length)
    ? ['部分 EC 数据读取失败：' + s.read_warnings.join('；')]
    : [];
  if (s.last_error) warnings.push('上次操作失败：' + s.last_error);
  const warning = warnings.join('　|　');
  ui.persistError.textContent = warning;
  ui.persistError.title = warning;
}

async function refresh(force = false) {
  if (refreshing) {
    if (force) pendingRefresh = true;
    return;
  }
  refreshing = true;
  try {
    const s = await invoke(IPC.getStatus);
    const now = Date.now();
    if (stats.lastAt !== null) stats.lastGapMs = now - stats.lastAt;
    stats.lastAt = now;
    stats.samples += 1;
    renderStatus(s);
  } catch (e) {
    setStatus('读取状态失败：' + e, 'err');
  } finally {
    refreshing = false;
    if (pendingRefresh) {
      pendingRefresh = false;
      refresh();
    }
  }
}

// ---------- 动作 ----------

/// 解析曲线输入（逗号分隔的整数），并做与后端一致的本地预校验.
function parseCurveInput() {
  const parse = (text, label) => {
    const out = (text || '').split(/[\s,]+/).filter(Boolean).map((s) => {
      const n = Number(s);
      if (!Number.isInteger(n) || n < 0 || n > 255) throw new Error(label + ' 含非法值: ' + s + '（需 0-255 整数）');
      return n;
    });
    return out;
  };
  const temps = parse(ui.curveTemps.value, '温度点');
  const duties = parse(ui.curveDuties.value, '占空比');
  if (temps.length !== duties.length) throw new Error('温度点数 ' + temps.length + ' 与占空比点数 ' + duties.length + ' 不等');
  if (!temps.length) throw new Error('曲线不能为空');
  if (temps.length > 16) throw new Error('曲线最多 16 个点');
  for (let i = 0; i < temps.length; i += 1) {
    if (temps[i] < 20 || temps[i] > 110) throw new Error('第 ' + i + ' 点温度 ' + temps[i] + '°C 超出 20-110');
    if (i > 0 && temps[i] < temps[i - 1]) throw new Error('温度点必须单调不减（第 ' + i + ' 点 ' + temps[i] + ' < ' + temps[i - 1] + '）');
    if (duties[i] < 64) throw new Error('第 ' + i + ' 点占空比 ' + duties[i] + ' 低于安全下界 64');
  }
  return { temps, duties };
}

async function doApplyCurve() {
  let args;
  try {
    args = parseCurveInput();
  } catch (e) {
    ui.curveWriteStatus.textContent = '输入错误：' + e.message;
    return;
  }
  const summary = '温度 ' + args.temps.join('/') + ' °C\n占空比 ' + args.duties.join('/') +
    '\n\n共 ' + args.temps.length + ' 个点；未填写的格子保留 EC 内原值。' +
    '\n写入 EC 曲线表（CPU），写入期间风扇行为会临时改变。\n失败会尝试自动回滚。确定继续吗？';
  if (!window.confirm(summary)) {
    ui.curveWriteStatus.textContent = '已取消。';
    return;
  }
  ui.applyCurve.disabled = true;
  setBusy(true);
  ui.curveWriteStatus.textContent = '写入中…（每格写后读回，转速异常会自动回滚）';
  try {
    const msg = await invoke(IPC.setFanCurve, {
      tempBlock: 0x0F00,
      dutyBlock: 0x0F20,
      temps: args.temps,
      duties: args.duties,
    });
    ui.curveWriteStatus.textContent = msg;
    await loadCurves(true);
  } catch (e) {
    // 复审 #4：不得无条件宣称"已自动回滚" —— 回滚失败只落日志，前端无从得知
    ui.curveWriteStatus.textContent = '写入失败，已尝试自动回滚（若日志显示回滚失败，请重启应用）：' + e;
    await loadCurves(true);
  } finally {
    ui.applyCurve.disabled = false;
    setBusy(false);
  }
}

async function doSwitchMode(mode) {
  if (!window.confirm('切换档位会写入 EC 的档位寄存器，可能改变风扇和功耗行为。确定继续吗？')) return;
  setBusy(true);
  setStatus('正在切换档位…', 'busy');
  try {
    setStatus(await invoke(IPC.setMode, { mode }), 'ok');
    await refresh(true);
  } catch (e) {
    setStatus('切换失败：' + e, 'err');
    await refresh(true);
  } finally {
    setBusy(false);
  }
}

async function doToggleFanBoost(enabled) {
  if (!window.confirm(enabled ? '开启强冷会立即提高风扇转速并增加噪声。确定开启吗？' : '确定关闭强冷吗？')) return;
  setBusy(true);
  setStatus(enabled ? '正在开启强冷…' : '正在关闭强冷…', 'busy');
  try {
    setStatus(await invoke(IPC.setFanBoost, { enabled }), 'ok');
    await refresh(true);
  } catch (e) {
    setStatus('强冷操作失败：' + e, 'err');
    ui.fanBoost.checked = !enabled;
    await refresh(true);
  } finally {
    setBusy(false);
  }
}

async function doApplyPower() {
  const cur = readSliders();
  if (!window.confirm(`将把默认功耗墙写为 PL1=${cur.pl1}W / PL2=${cur.pl2}W / PL4=${cur.pl4}W。\n确定写入 EC 吗？`)) return;
  setBusy(true);
  setStatus('正在写入功耗墙…', 'busy');
  try {
    const msg = await invoke(IPC.setPowerLimits, {
      mode: activePowerMode, pl1: cur.pl1, pl2: cur.pl2, pl4: cur.pl4,
    });
    setStatus(msg, 'ok');
    editingPower = false;
    ui.pending.dataset.dirty = 'false';
    await refresh(true);
  } catch (e) {
    setStatus('写入失败：' + e, 'err');
    await refresh(true);
  } finally {
    setBusy(false);
  }
}

const UNDO_COMMANDS = { mode: 'undoMode', power: 'undoPower', fan: 'undoFanBoost' };

async function doUndo(which) {
  setBusy(true);
  setStatus('正在撤销…', 'busy');
  try {
    setStatus(await invoke(IPC[UNDO_COMMANDS[which]]), 'ok');
    await refresh(true);
  } catch (e) {
    setStatus('撤销失败：' + e, 'err');
    await refresh(true);
  } finally {
    setBusy(false);
  }
}

// ---------- 配置 ----------

function renderProfiles(list) {
  ui.profileList.textContent = '';
  if (!list || list.length === 0) {
    const li = document.createElement('li');
    li.className = 'empty';
    li.textContent = '还没有自定义配置';
    ui.profileList.appendChild(li);
    return;
  }
  for (const p of list) {
    const li = document.createElement('li');
    li.className = 'profile-item';
    li.dataset.profile = p.name;

    const info = document.createElement('div');
    info.className = 'profile-info';
    const name = document.createElement('strong');
    name.textContent = p.name;
    const meta = document.createElement('span');
    meta.className = 'mono';
    meta.textContent = 'PL1=' + p.pl1 + ' PL2=' + p.pl2 + ' PL4=' + p.pl4 + (p.fan_boost ? ' · 强冷' : '');
    info.append(name, meta);

    const actions = document.createElement('div');
    actions.className = 'profile-actions';
    const apply = document.createElement('button');
    apply.className = 'btn';
    apply.textContent = '应用';
    apply.dataset.action = 'apply';
    apply.addEventListener('click', () => doApplyProfile(p.name));
    const del = document.createElement('button');
    del.className = 'btn subtle';
    del.textContent = '删除';
    del.dataset.action = 'delete';
    del.addEventListener('click', () => doDeleteProfile(p.name));
    actions.append(apply, del);

    li.append(info, actions);
    ui.profileList.appendChild(li);
  }
}

async function loadProfiles() {
  try {
    const payload = await invoke(IPC.listProfiles);
    if (payload && payload.kind === 'needs_attention') {
      ui.profileList.textContent = '';
      const li = document.createElement('li');
      li.className = 'empty';
      li.textContent = payload.message;
      if (payload.backup) {
        const restore = document.createElement('button');
        restore.className = 'btn subtle';
        restore.textContent = '从备份恢复';
        restore.addEventListener('click', doRestoreProfiles);
        li.appendChild(document.createTextNode(' '));
        li.appendChild(restore);
      }
      ui.profileList.appendChild(li);
      setStatus(payload.message, 'err');
      return;
    }
    renderProfiles(payload && payload.kind === 'ready' ? payload.profiles : payload);
  } catch (e) {
    setStatus('读取配置失败：' + e, 'err');
  }
}

async function doRestoreProfiles() {
  if (!window.confirm('将从 profiles.json.broken 恢复配置；当前 profiles.json 会先保存为 .bak。确定继续吗？')) return;
  setBusy(true);
  try {
    renderProfiles(await invoke(IPC.restoreProfiles));
    setStatus('已从备份恢复配置', 'ok');
  } catch (e) {
    setStatus('恢复失败：' + e, 'err');
  } finally {
    setBusy(false);
  }
}

async function doSaveProfile() {
  const name = (ui.profileName.value || '').trim();
  if (!name) {
    setStatus('请先填写配置名', 'err');
    ui.profileName.focus();
    return;
  }
  const cur = readSliders();
  setBusy(true);
  try {
    renderProfiles(await invoke(IPC.saveProfile, {
      profile: { name, pl1: cur.pl1, pl2: cur.pl2, pl4: cur.pl4, fan_boost: ui.fanBoost.checked },
    }));
    ui.profileName.value = '';
    setStatus('已保存配置「' + name + '」', 'ok');
  } catch (e) {
    setStatus('保存失败：' + e, 'err');
  } finally {
    setBusy(false);
  }
}

async function doDeleteProfile(name) {
  setBusy(true);
  try {
    renderProfiles(await invoke(IPC.deleteProfile, { name }));
    setStatus('已删除配置「' + name + '」', 'ok');
  } catch (e) {
    setStatus('删除失败：' + e, 'err');
  } finally {
    setBusy(false);
  }
}

async function doApplyProfile(name) {
  if (!window.confirm(`配置「${name}」会写入默认功耗墙，且可能开启强冷。确定应用吗？`)) return;
  setBusy(true);
  setStatus('正在应用「' + name + '」…', 'busy');
  try {
    setStatus(await invoke(IPC.applyProfile, { name }), 'ok');
    await refresh(true);
  } catch (e) {
    setStatus('应用失败：' + e, 'err');
    await refresh(true);
  } finally {
    setBusy(false);
  }
}

// ---------- 风扇曲线（自动加载） ----------

function curveChart(points) {
  const W = 320;
  const H = 140;
  const padL = 32;
  const padR = 10;
  const padT = 12;
  const padB = 22;
  if (!points.length) return '';
  const temps = points.map((p) => p.temp_c);
  const minT = Math.min.apply(null, temps);
  const maxT = Math.max.apply(null, temps);
  const spanT = maxT - minT || 1;
  const x = (t) => padL + ((t - minT) / spanT) * (W - padL - padR);
  const y = (d) => H - padB - (Math.max(0, Math.min(100, d)) / 100) * (H - padT - padB);

  const line = points.map((p, i) => (i === 0 ? 'M' : 'L') + x(p.temp_c).toFixed(1) + ',' + y(p.duty_pct).toFixed(1)).join(' ');
  const last = points[points.length - 1];
  const first = points[0];
  const area = line + ' L' + x(last.temp_c).toFixed(1) + ',' + y(0).toFixed(1) + ' L' + x(first.temp_c).toFixed(1) + ',' + y(0).toFixed(1) + ' Z';
  const dots = points.map((p) => '<circle class="curve-dot" cx="' + x(p.temp_c).toFixed(1) + '" cy="' + y(p.duty_pct).toFixed(1) + '" r="3"></circle>').join('');

  const grid = [0, 25, 50, 75, 100].map((v) => {
    const gy = y(v);
    return '<line class="grid-line" x1="' + padL + '" y1="' + gy.toFixed(1) + '" x2="' + (W - padR) + '" y2="' + gy.toFixed(1) + '"></line>' +
      '<text class="axis-text" x="' + (padL - 5) + '" y="' + (gy + 3).toFixed(1) + '" text-anchor="end">' + v + '%</text>';
  }).join('');

  const ticks = [minT, Math.round((minT + maxT) / 2), maxT].map((t) =>
    '<text class="axis-text" x="' + x(t).toFixed(1) + '" y="' + (H - 6) + '" text-anchor="middle">' + t + '°C</text>').join('');

  return '<svg class="curve-svg" viewBox="0 0 ' + W + ' ' + H + '" role="img" aria-label="风扇曲线：' + points.length +
    ' 个控制点，温度 ' + minT + ' 到 ' + maxT + ' 摄氏度">' +
    grid + ticks +
    '<line class="axis-line" x1="' + padL + '" y1="' + padT + '" x2="' + padL + '" y2="' + (H - padB) + '"></line>' +
    '<line class="axis-line" x1="' + padL + '" y1="' + (H - padB) + '" x2="' + (W - padR) + '" y2="' + (H - padB) + '"></line>' +
    '<path class="curve-area" d="' + area + '"></path>' +
    '<path class="curve-line" d="' + line + '"></path>' +
    dots + '</svg>';
}

function renderCurves(data) {
  ui.curveBody.textContent = '';

  const grid = document.createElement('div');
  grid.className = 'curve-grid';
  for (const c of data.curves) {
    const card = document.createElement('div');
    card.className = 'curve-card';
    const h = document.createElement('h3');
    h.textContent = c.label;
    const meta = document.createElement('div');
    meta.className = 'meta';
    const temps = c.points.map((p) => p.temp_c);
    meta.textContent = c.points.length + ' 个控制点 · 温度 ' + Math.min.apply(null, temps) + '–' + Math.max.apply(null, temps) + ' °C';
    card.append(h, meta);
    card.insertAdjacentHTML('beforeend', curveChart(c.points));
    grid.appendChild(card);
  }
  ui.curveBody.appendChild(grid);

  const table = document.createElement('div');
  table.className = 'curve-table';
  for (const b of data.blocks) {
    const row = document.createElement('div');
    const addr = document.createElement('b');
    addr.textContent = '0x' + b.base.toString(16).toUpperCase().padStart(4, '0');
    const kind = document.createElement('span');
    kind.className = 'curve-kind';
    kind.dataset.kind = b.kind;
    kind.textContent = { temperature_points: '温度点', duty_percent: '转速档', unknown: '未识别' }[b.kind] || b.kind;
    const values = document.createElement('span');
    values.textContent = '  ' + b.values.join(', ');
    row.append(addr, kind, values);
    table.appendChild(row);
  }
  ui.curveBody.appendChild(table);

  ui.curveMeta.textContent = '已读取 ' + data.blocks.length + ' 张表 · ' + new Date().toLocaleTimeString('zh-CN');
  curvesLoadedAt = Date.now();
}

async function loadCurves(force) {
  const now = Date.now();
  if (!force && now - curvesLoadedAt < CURVE_REFRESH_MS) return;
  if (!force && now < curveRetryAfter) return;
  ui.reloadCurves.disabled = true;
  try {
    renderCurves(await invoke(IPC.getFanCurves));
    curveRetryAfter = 0;
  } catch (e) {
    curveRetryAfter = now + CURVE_RETRY_MS;
    ui.curveMeta.textContent = '读取失败：' + e + '（30 秒后可重试，或点重新读取）';
    if (!curvesLoadedAt) {
      ui.curveBody.textContent = '暂时无法读取风扇曲线。请检查 EC 驱动与管理员权限，然后重试。';
    }
  } finally {
    ui.reloadCurves.disabled = false;
  }
}

// ---------- 寄存器浏览器 ----------

function parseAddrInput(text) {
  const out = [];
  for (const token of (text || '').split(/[\s,]+/).filter(Boolean)) {
    const range = token.match(/^(?:0x)?([0-9a-f]{1,4})-(?:0x)?([0-9a-f]{1,4})$/i);
    if (range) {
      const lo = parseInt(range[1], 16);
      const hi = parseInt(range[2], 16);
      if (hi < lo) throw new Error('区间 ' + token + ' 上界小于下界');
      if (hi - lo > 63) throw new Error('区间 ' + token + ' 太大（一次最多 ' + MAX_REG_QUERY + ' 个地址）');
      for (let a = lo; a <= hi; a += 1) out.push(a);
      continue;
    }
    const single = token.match(/^(?:0x)?([0-9a-f]{1,4})$/i);
    if (!single) throw new Error('看不懂的地址：' + token);
    out.push(parseInt(single[1], 16));
  }
  const bad = out.find((a) => a > MAX_EC_ADDR);
  if (bad !== undefined) throw new Error('地址 0x' + bad.toString(16).toUpperCase() + ' 超出 EC 范围 0x0000-0x0FFF');
  if (out.length > MAX_REG_QUERY) throw new Error('一次最多读 ' + MAX_REG_QUERY + ' 个地址（当前 ' + out.length + ' 个）');
  return out;
}

async function doReadRegs() {
  let addrs;
  try {
    addrs = parseAddrInput(ui.regAddrs.value);
  } catch (e) {
    setStatus(String(e.message || e), 'err');
    return;
  }
  if (addrs.length === 0) {
    setStatus('请先填写要读取的地址', 'err');
    return;
  }
  setBusy(true);
  setStatus('正在读取 ' + addrs.length + ' 个地址…', 'busy');
  try {
    const rows = await invoke(IPC.readRegisters, { addrs });
    lastRegRead = rows;
    ui.regOut.textContent = rows
      .map((r) => '0x' + r.addr.toString(16).toUpperCase().padStart(4, '0') + '   ' + String(r.value).padStart(3, ' ') + '   ' + hex2(r.value))
      .join('\n');
    ui.exportRegs.disabled = false;
    setStatus('已读取 ' + rows.length + ' 个寄存器', 'ok');
  } catch (e) {
    setStatus('读取失败：' + e, 'err');
    ui.regOut.textContent = String(e);
  } finally {
    setBusy(false);
  }
}

function doExportRegs() {
  if (!lastRegRead || lastRegRead.length === 0) return;
  const lines = ['addr_hex,value_dec,value_hex'];
  for (const r of lastRegRead) {
    lines.push('0x' + r.addr.toString(16).toUpperCase().padStart(4, '0') + ',' + r.value + ',' + hex2(r.value));
  }
  const blob = new Blob([lines.join('\n')], { type: 'text/csv;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'mechrevo-registers-' + new Date().toISOString().slice(0, 19).replace(/[:T]/g, '') + '.csv';
  a.click();
  URL.revokeObjectURL(url);
  setStatus('寄存器已导出 CSV', 'ok');
}

// ---------- 路由 ----------

const PAGES = ['overview', 'monitor', 'device', 'battery', 'performance', 'cooling', 'advanced'];

function showPage(name) {
  const page = PAGES.indexOf(name) >= 0 ? name : 'overview';
  for (const section of document.querySelectorAll('.page')) {
    if (section.dataset.page === page) section.setAttribute('data-active', '');
    else section.removeAttribute('data-active');
  }
  for (const btn of ui.nav.querySelectorAll('.nav-item')) {
    if (btn.dataset.page === page) btn.setAttribute('aria-current', 'page');
    else btn.removeAttribute('aria-current');
  }
  if (location.hash !== '#' + page) location.hash = page;
  if (page === 'cooling') loadCurves();
}

// ---------- 事件绑定 ----------

ui.nav.addEventListener('click', (ev) => {
  const btn = ev.target.closest('.nav-item');
  if (btn) showPage(btn.dataset.page);
});

window.addEventListener('hashchange', () => showPage(location.hash.replace('#', '')));

ui.modeList.addEventListener('click', (ev) => {
  const btn = ev.target.closest('.mode-btn');
  if (btn && ecReady) doSwitchMode(btn.dataset.mode);
});

ui.fanBoost.addEventListener('change', () => {
  if (ecReady) doToggleFanBoost(ui.fanBoost.checked);
});

ui.powerTabs.addEventListener('click', (ev) => {
  const tab = ev.target.closest('.tab');
  if (!tab) return;
  const nextMode = tab.dataset.powerMode;
  if (nextMode === activePowerMode) return;
  if (ui.pending.dataset.dirty === 'true') {
    const ok = window.confirm('当前模式有未应用的功耗改动。\n切换将丢弃这些改动，确定继续吗？');
    if (!ok) return;
  }
  activePowerMode = nextMode;
  for (const t of ui.powerTabs.querySelectorAll('.tab')) {
    t.setAttribute('aria-selected', t === tab ? 'true' : 'false');
  }
  editingPower = false;
  setSliders(baseline[activePowerMode]);
  updateDirty();
});

[ui.pl1, ui.pl2, ui.pl4].forEach((input) => {
  input.addEventListener('input', syncSliderOutputs);
  input.addEventListener('pointerdown', () => { editingPower = true; });
  input.addEventListener('pointerup', () => { editingPower = false; });
  input.addEventListener('pointercancel', () => { editingPower = false; });
  input.addEventListener('focus', () => { editingPower = true; });
  input.addEventListener('blur', () => { editingPower = false; });
  input.addEventListener('keydown', () => { editingPower = true; });
  input.addEventListener('pointermove', (ev) => { if (editingPower) ev.preventDefault(); });
});

window.addEventListener('blur', () => { editingPower = false; });

ui.applyPower.addEventListener('click', doApplyPower);
ui.undoPower.addEventListener('click', () => doUndo('power'));
ui.undoMode.addEventListener('click', () => doUndo('mode'));
ui.undoFan.addEventListener('click', () => doUndo('fan'));
ui.resetPower.addEventListener('click', () => {
  editingPower = false;
  setSliders(baseline[activePowerMode]);
  setStatus('已重置为读回值');
});
ui.resetStats.addEventListener('click', () => {
  resetStats();
  setStatus('统计已清零');
});
ui.saveProfile.addEventListener('click', doSaveProfile);
ui.readRegs.addEventListener('click', doReadRegs);
ui.exportRegs.addEventListener('click', doExportRegs);
ui.regAddrs.addEventListener('keydown', (ev) => { if (ev.key === 'Enter') doReadRegs(); });
ui.reloadCurves.addEventListener('click', () => loadCurves(true));
ui.applyCurve.addEventListener('click', doApplyCurve);

ui.relaunch.addEventListener('click', async () => {
  setBusy(true);
  setStatus('正在请求管理员权限…', 'busy');
  try {
    await invoke(IPC.relaunchElevated);
  } catch (e) {
    setStatus('提权失败：' + e, 'err');
    setBusy(false);
  }
});

// ---------- 启动 ----------

function tickClock() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  ui.clock.textContent = p(d.getHours()) + ':' + p(d.getMinutes()) + ':' + p(d.getSeconds());
}

function boot() {
  const missing = findMissingElements(ui);
  if (missing.length > 0) {
    setStatus('界面元素缺失: ' + missing.join(', '), 'err');
    console.error('[mechrevo-max]', '界面元素缺失: ' + missing.join(', '), {
      hint: 'app.js 里的 ui 映射与 index.html 的 id 对不上',
    });
    return;
  }

  for (const t of ui.powerTabs.querySelectorAll('.tab')) {
    t.setAttribute('aria-selected', t.dataset.powerMode === activePowerMode ? 'true' : 'false');
  }
  syncSliderOutputs();
  renderSampleInfo();
  showPage(location.hash.replace('#', '') || 'overview');

  refresh();
  startPolling();
  loadProfiles();
  loadCurves(); // 自动读一次，不再需要点按钮
  setInterval(tickClock, 1000);
  tickClock();
}

document.addEventListener('DOMContentLoaded', boot);
