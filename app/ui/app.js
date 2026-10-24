// 机械革命控制台 · 前端逻辑
// 与 Rust 后端通过 Tauri IPC 通信

const { invoke } = window.__TAURI__.core;

/**
 * IPC 契约表 —— **后端命令名的唯一来源**。
 *
 * `ipc_contract.rs` 会在编译期解析本表，与后端注册的命令集做双向比对。
 * 约定：任何 invoke 都必须经由本表（测试会检查源码里没有直接写字符串命令名的调用）。
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
  saveProfile: 'save_profile',
  deleteProfile: 'delete_profile',
  applyProfile: 'apply_profile',
  getFanCurves: 'get_fan_curves',
  relaunchElevated: 'relaunch_elevated',
};

const POLL_MS = 2000;
const TEMP_WARM = 60;
const TEMP_HOT = 80;
const MAX_EC_ADDR = 0x0fff;
const MAX_REG_QUERY = 64;

const el = (id) => document.getElementById(id);

function findMissingElements(map) {
  return Object.entries(map)
    .filter(([, node]) => !node)
    .map(([name]) => name);
}

const ui = {
  ecPill: el('ec-pill'),
  ecText: el('ec-text'),
  projectId: el('project-id'),
  sampleInfo: el('sample-info'),
  alert: el('ec-alert'),
  alertTitle: el('ec-alert-title'),
  alertMsg: el('ec-alert-msg'),
  alertDriver: el('alert-driver'),
  relaunch: el('relaunch'),
  modeList: el('mode-list'),
  modeRaw: el('mode-raw'),
  bfMode: el('bf-mode'),
  bfFanBoost: el('bf-fanboost'),
  modeSource: el('mode-source'),
  fanBoost: el('fan-boost'),
  undoMode: el('undo-mode'),
  undoFan: el('undo-fan'),
  sensorTable: el('sensor-table'),
  sensorsMeta: el('sensors-meta'),
  statsHint: el('stats-hint'),
  resetStats: el('reset-stats'),
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
  profileName: el('profile-name'),
  saveProfile: el('save-profile'),
  profileList: el('profile-list'),
  profilePath: el('profile-path'),
  regAddrs: el('reg-addrs'),
  readRegs: el('read-regs'),
  exportRegs: el('export-regs'),
  regOut: el('reg-out'),
  fwPl: el('fw-pl'),
  loadCurves: el('load-curves'),
  curveBody: el('curve-body'),
  logPath: el('log-path'),
  statusMsg: el('status-msg'),
  persistError: el('persist-error'),
  alertLive: el('alert-live'),
  clock: el('clock'),
};

/** 当前选中的功耗模式 tab */
let activePowerMode = 'gaming';
/** 最后一次从硬件读回的功耗值，用于"重置"与脏检查 */
let baseline = { gaming: null, office: null };
let ecReady = false;
let pollTimer = null;
let refreshing = false;
let pendingRefresh = false;
let editingPower = false;
let lastStatus = null;

/** 采样统计 */
const stats = { samples: 0, lastAt: null, lastGapMs: null, byKey: {} };
/** 最近一次寄存器读取结果（导出 CSV 用） */
let lastRegRead = null;

// ---------- 工具 ----------

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

function fmt(v, fallback = '—') {
  return v === null || v === undefined ? fallback : String(v);
}

function paintRange(input) {
  const min = Number(input.min);
  const max = Number(input.max);
  const val = Number(input.value);
  const pct = max > min ? ((val - min) / (max - min)) * 100 : 0;
  input.style.setProperty('--fill', `${pct}%`);
}

function readSliders() {
  return { pl1: Number(ui.pl1.value), pl2: Number(ui.pl2.value), pl4: Number(ui.pl4.value) };
}

function setSliders(limits) {
  if (!limits) return;
  ui.pl1.value = limits.pl1;
  ui.pl2.value = limits.pl2;
  ui.pl4.value = limits.pl4;
  syncSliderOutputs();
}

const SLIDER_SEMANTICS = [
  { input: 'pl1', out: 'pl1Out', label: '长时功耗墙 PL1' },
  { input: 'pl2', out: 'pl2Out', label: '短时功耗墙 PL2' },
  { input: 'pl4', out: 'pl4Out', label: '峰值功耗墙 PL4' },
];

function syncSliderOutputs() {
  for (const { input, out, label } of SLIDER_SEMANTICS) {
    const node = ui[input];
    ui[out].textContent = `${node.value} W`;
    node.setAttribute('aria-valuetext', `${node.value} 瓦，${label}`);
    paintRange(node);
  }
  updateDirty();
}

function updateDirty() {
  const base = baseline[activePowerMode];
  const cur = readSliders();
  const dirty = base ? cur.pl1 !== base.pl1 || cur.pl2 !== base.pl2 || cur.pl4 !== base.pl4 : false;
  ui.pending.textContent = dirty ? '有未应用的修改' : '未修改';
  ui.pending.dataset.dirty = dirty ? 'true' : 'false';
  ui.applyPower.disabled = !ecReady || !dirty;
}

// ---------- 采样统计 ----------

function recordSample(key, value) {
  if (value === null || value === undefined) return;
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
  renderSensors(lastStatus);
}

function renderSampleInfo() {
  const gap = stats.lastGapMs === null ? '—' : `${(stats.lastGapMs / 1000).toFixed(1)}s`;
  const since = stats.lastAt === null ? '—' : `${Math.round((Date.now() - stats.lastAt) / 1000)}s 前`;
  ui.sampleInfo.textContent = `间隔 ${(POLL_MS / 1000).toFixed(1)}s · 实测 ${gap} · ${stats.samples} 次`;
  ui.sampleInfo.title = `上次刷新：${since}`;
  ui.statsHint.textContent = `本次启动已采样 ${stats.samples} 次 · 上次 ${since}`;
}

// ---------- 渲染 ----------

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
  ui.alertLive.textContent = `${ui.alertTitle.textContent}：${ec.message}`;

  const needsElevation = ec.hint === 'elevation';
  const needsRestart = ec.hint === 'restart';
  ui.relaunch.hidden = !(needsElevation || needsRestart);
  ui.alertDriver.hidden = needsElevation || needsRestart;
  ui.alertTitle.textContent = needsElevation
    ? '需要管理员权限'
    : needsRestart
      ? '需要重启程序'
      : 'EC 通道不可用';
}

function renderMode(mode) {
  for (const btn of ui.modeList.querySelectorAll('.mode-btn')) {
    btn.setAttribute('aria-pressed', btn.dataset.mode === mode ? 'true' : 'false');
    btn.disabled = !ecReady;
  }
}

const hex2 = (v) => `0x${v.toString(16).toUpperCase().padStart(2, '0')}`;
const modeLabel = (key) => ({ balance: '均衡', office: '办公', turbo: '狂暴' })[key] || key;

/**
 * 把 0x751 的位域拆开显示 —— "黑匣子"里最该解释清楚的一格。
 */
function renderBitfield(raw, mode) {
  if (!raw) {
    ui.modeRaw.textContent = '751=— · 7AB=—';
    ui.modeRaw.setAttribute('aria-label', '档位原始寄存器：无数据');
    ui.bfMode.textContent = '—';
    ui.bfFanBoost.textContent = '—';
    return;
  }
  const [a, b] = raw;
  ui.modeRaw.textContent = `751=${hex2(a)} · 7AB=${hex2(b)}`;
  ui.modeRaw.setAttribute('aria-label', `档位原始寄存器：751 为 ${a}，7AB 为 ${b}`);
  ui.bfMode.textContent = mode
    ? `${hex2(a & 0xbf)} → ${modeLabel(mode)}`
    : `${hex2(a & 0xbf)} → 未知编码（不猜测）`;
  ui.bfFanBoost.textContent = `${(a & 0x40) !== 0 ? '开' : '关'}（${hex2(a & 0x40)}）`;
}

function sensorState(key, value) {
  if (value === null || value === undefined) return '无数据';
  if (key === 'cpu_temp' || key === 'gpu_temp') {
    if (value >= TEMP_HOT) return '过热';
    if (value >= TEMP_WARM) return '偏高';
    return '正常';
  }
  if (key === 'cpu_fan' || key === 'gpu_fan') {
    if (value === 0) return '静止';
    return value >= 4500 ? '高转速' : '运转中';
  }
  return '—';
}

function sensorUnit(key) {
  if (key === 'cpu_temp' || key === 'gpu_temp') return ' °C';
  if (key === 'cpu_fan' || key === 'gpu_fan') return ' RPM';
  return '';
}

function renderSensors(s) {
  if (!s) return;
  const ready = s.ec.kind === 'ready';
  const rows = {
    cpu_temp: ready ? s.cpu_temp_c : null,
    gpu_temp: ready ? s.gpu_temp_c : null,
    cpu_fan: ready ? s.cpu_fan_rpm : null,
    gpu_fan: ready ? s.gpu_fan_rpm : null,
    power_src: ready ? (s.is_ac_powered ? '适配器' : '电池') : null,
  };

  for (const [key, value] of Object.entries(rows)) {
    const tr = ui.sensorTable.querySelector(`tr[data-sensor="${key}"]`);
    if (!tr) continue;
    const unit = sensorUnit(key);
    const numeric = typeof value === 'number';
    tr.querySelector('[data-col="cur"]').textContent =
      value === null || value === undefined ? '—' : `${value}${unit}`;
    for (const col of ['min', 'avg', 'max']) {
      const v = numeric ? statOf(key, col) : null;
      tr.querySelector(`[data-col="${col}"]`).textContent =
        v === null ? '—' : `${Math.round(v)}${unit}`;
    }
    tr.querySelector('[data-col="state"]').textContent = sensorState(key, value);
    tr.dataset.level = numeric && key.includes('temp')
      ? (value >= TEMP_HOT ? 'hot' : value >= TEMP_WARM ? 'warm' : 'ok')
      : 'ok';
  }

  ui.sensorsMeta.textContent = ready
    ? `当前 / 最小 / 平均 / 最大 · 采样 ${stats.samples} 次`
    : 'EC 不可用';
}

function renderPower(s) {
  baseline.gaming = s.gaming_power;
  baseline.office = s.office_power;

  const dirty = ui.pending.dataset.dirty === 'true';
  if (!editingPower && !dirty) setSliders(baseline[activePowerMode]);
  updateDirty();

  if (ui.fwPl) {
    const fw = s.firmware_power;
    ui.fwPl.textContent = fw
      ? `PL1 ${fw.pl1} W · PL2 ${fw.pl2} W · PL4 ${fw.pl4} W · 0x786=${fw.extra}`
      : '—';
  }

  ui.undoMode.disabled = !ecReady || !s.can_undo_mode;
  ui.undoPower.disabled = !ecReady || !s.can_undo_power;
  ui.undoFan.disabled = !ecReady || !s.can_undo_fan_boost;
}

function renderPersistError(message) {
  ui.persistError.hidden = !message;
  ui.persistError.textContent = message ? `上次操作失败：${message}` : '';
  ui.persistError.title = message || '';
}

function renderStatus(s) {
  lastStatus = s;
  renderEc(s.ec);
  ui.projectId.textContent = fmt(s.project_id);
  renderMode(s.mode);
  renderBitfield(s.mode_raw, s.mode);

  if (s.fan_boost !== null && s.fan_boost !== undefined && document.activeElement !== ui.fanBoost) {
    ui.fanBoost.checked = !!s.fan_boost;
  }
  ui.fanBoost.disabled = !ecReady;

  renderSensors(s);
  renderSampleInfo();
  renderPower(s);
  renderPersistError(s.last_error);
}

// ---------- 行为 ----------

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
    if (s.ec.kind === 'ready') {
      recordSample('cpu_temp', s.cpu_temp_c);
      recordSample('gpu_temp', s.gpu_temp_c);
      recordSample('cpu_fan', s.cpu_fan_rpm);
      recordSample('gpu_fan', s.gpu_fan_rpm);
    }
    renderStatus(s);
  } catch (e) {
    setStatus(`读取状态失败：${e}`, 'err');
  } finally {
    refreshing = false;
    if (pendingRefresh) {
      pendingRefresh = false;
      refresh();
    }
  }
}

async function doSwitchMode(mode) {
  setBusy(true);
  setStatus('正在切换档位…', 'busy');
  try {
    setStatus(await invoke(IPC.setMode, { mode }), 'ok');
    await refresh(true);
  } catch (e) {
    setStatus(`切换失败：${e}`, 'err');
    await refresh(true);
  } finally {
    setBusy(false);
  }
}

async function doToggleFanBoost(enabled) {
  setBusy(true);
  setStatus(enabled ? '正在开启强冷…' : '正在关闭强冷…', 'busy');
  try {
    setStatus(await invoke(IPC.setFanBoost, { enabled }), 'ok');
    await refresh(true);
  } catch (e) {
    setStatus(`强冷操作失败：${e}`, 'err');
    ui.fanBoost.checked = !enabled;
    await refresh(true);
  } finally {
    setBusy(false);
  }
}

async function doApplyPower() {
  const cur = readSliders();
  setBusy(true);
  setStatus('正在写入功耗墙…', 'busy');
  try {
    const msg = await invoke(IPC.setPowerLimits, {
      mode: activePowerMode,
      pl1: cur.pl1,
      pl2: cur.pl2,
      pl4: cur.pl4,
    });
    setStatus(msg, 'ok');
    editingPower = false;
    ui.pending.dataset.dirty = 'false';
    await refresh(true);
  } catch (e) {
    setStatus(`写入失败：${e}`, 'err');
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
    setStatus(`撤销失败：${e}`, 'err');
    await refresh(true);
  } finally {
    setBusy(false);
  }
}

// ---------- 自定义配置 ----------

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
    meta.textContent = `PL1=${p.pl1} PL2=${p.pl2} PL4=${p.pl4}${p.fan_boost ? ' · 强冷' : ''}`;
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
    renderProfiles(await invoke(IPC.listProfiles));
  } catch (e) {
    setStatus(`读取配置失败：${e}`, 'err');
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
    const list = await invoke(IPC.saveProfile, {
      profile: { name, pl1: cur.pl1, pl2: cur.pl2, pl4: cur.pl4, fan_boost: ui.fanBoost.checked },
    });
    renderProfiles(list);
    ui.profileName.value = '';
    setStatus(`已保存配置「${name}」`, 'ok');
  } catch (e) {
    setStatus(`保存失败：${e}`, 'err');
  } finally {
    setBusy(false);
  }
}

async function doDeleteProfile(name) {
  setBusy(true);
  try {
    renderProfiles(await invoke(IPC.deleteProfile, { name }));
    setStatus(`已删除配置「${name}」`, 'ok');
  } catch (e) {
    setStatus(`删除失败：${e}`, 'err');
  } finally {
    setBusy(false);
  }
}

async function doApplyProfile(name) {
  setBusy(true);
  setStatus(`正在应用「${name}」…`, 'busy');
  try {
    setStatus(await invoke(IPC.applyProfile, { name }), 'ok');
    await refresh(true);
  } catch (e) {
    setStatus(`应用失败：${e}`, 'err');
    await refresh(true);
  } finally {
    setBusy(false);
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
      if (hi < lo) throw new Error(`区间 ${token} 上界小于下界`);
      if (hi - lo > 63) throw new Error(`区间 ${token} 太大（一次最多 64 个地址）`);
      for (let a = lo; a <= hi; a += 1) out.push(a);
      continue;
    }
    const single = token.match(/^(?:0x)?([0-9a-f]{1,4})$/i);
    if (!single) throw new Error(`看不懂的地址：${token}`);
    out.push(parseInt(single[1], 16));
  }
  // 与后端一致的前置校验：EC 地址只到 0x0FFF，别等往返一次才被拒
  const bad = out.find((a) => a > MAX_EC_ADDR);
  if (bad !== undefined) {
    throw new Error(`地址 0x${bad.toString(16).toUpperCase()} 超出 EC 范围 0x0000-0x0FFF`);
  }
  if (out.length > MAX_REG_QUERY) {
    throw new Error(`一次最多读 ${MAX_REG_QUERY} 个地址（当前 ${out.length} 个）`);
  }
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
  setStatus(`正在读取 ${addrs.length} 个地址…`, 'busy');
  try {
    const rows = await invoke(IPC.readRegisters, { addrs });
    lastRegRead = rows;
    ui.regOut.textContent = rows
      .map(
        (r) =>
          `0x${r.addr.toString(16).toUpperCase().padStart(4, '0')}   ${String(r.value).padStart(3, ' ')}   ${hex2(r.value)}`,
      )
      .join('\n');
    ui.exportRegs.disabled = false;
    setStatus(`已读取 ${rows.length} 个寄存器`, 'ok');
  } catch (e) {
    setStatus(`读取失败：${e}`, 'err');
    ui.regOut.textContent = String(e);
  } finally {
    setBusy(false);
  }
}

function doExportRegs() {
  if (!lastRegRead || lastRegRead.length === 0) return;
  const lines = ['addr_hex,value_dec,value_hex'];
  for (const r of lastRegRead) {
    lines.push(`0x${r.addr.toString(16).toUpperCase().padStart(4, '0')},${r.value},${hex2(r.value)}`);
  }
  const blob = new Blob([lines.join('\n')], { type: 'text/csv;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `mechrevo-registers-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, '')}.csv`;
  a.click();
  URL.revokeObjectURL(url);
  setStatus('寄存器已导出 CSV', 'ok');
}

// ---------- 风扇曲线（只读） ----------

function svgPolyline(points, width, height) {
  if (!points.length) return '';
  const xs = points.map((p) => p.temp_c);
  const ys = points.map((p) => p.duty_pct);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minY = 0;
  const maxY = 100;
  const sx = (x) => (maxX === minX ? width / 2 : ((x - minX) / (maxX - minX)) * (width - 16) + 8);
  const sy = (y) => height - 10 - ((y - minY) / (maxY - minY)) * (height - 20);
  const pts = points.map((p) => `${sx(p.temp_c).toFixed(1)},${sy(p.duty_pct).toFixed(1)}`).join(' ');
  const dots = points
    .map((p) => `<circle cx="${sx(p.temp_c).toFixed(1)}" cy="${sy(p.duty_pct).toFixed(1)}" r="2.5"></circle>`)
    .join('');
  return `<svg class="curve-svg" viewBox="0 0 ${width} ${height}" preserveAspectRatio="none" role="img"
      aria-label="温度到占空比曲线，${points.length} 个点">
      <line x1="8" y1="${height - 10}" x2="${width - 8}" y2="${height - 10}"></line>
      <line x1="8" y1="10" x2="8" y2="${height - 10}"></line>
      ${dots}<polyline points="${pts}"></polyline>
    </svg>`;
}

function renderCurves(data) {
  ui.curveBody.textContent = '';

  const blocks = document.createElement('div');
  blocks.className = 'curve-blocks';
  for (const b of data.blocks) {
    const row = document.createElement('div');
    row.className = 'curve-block';
    const addr = document.createElement('b');
    addr.textContent = `0x${b.base.toString(16).toUpperCase().padStart(4, '0')}`;
    const kind = document.createElement('span');
    kind.className = 'curve-kind';
    kind.dataset.kind = b.kind;
    kind.textContent = { temperature_points: '温度点', duty_percent: '占空比', unknown: '未知' }[b.kind] || b.kind;
    const vals = document.createElement('span');
    vals.textContent = b.values.join(', ');
    row.append(addr, kind, vals);
    blocks.appendChild(row);
  }
  ui.curveBody.appendChild(blocks);

  const grid = document.createElement('div');
  grid.className = 'curve-grid';
  for (const c of data.curves) {
    const card = document.createElement('div');
    card.className = 'curve-card';
    const h = document.createElement('h3');
    h.textContent = c.label;
    const meta = document.createElement('div');
    meta.className = 'mono';
    meta.textContent = `温度块 0x${c.temp_block.toString(16).toUpperCase()} → 占空比块 0x${c.duty_block.toString(16).toUpperCase()}`;
    card.append(h, meta);
    card.insertAdjacentHTML('beforeend', svgPolyline(c.points, 240, 96));
    grid.appendChild(card);
  }
  ui.curveBody.appendChild(grid);

  const note = document.createElement('p');
  note.className = 'hint';
  note.textContent = data.note;
  ui.curveBody.appendChild(note);
}

async function doLoadCurves() {
  setBusy(true);
  setStatus('正在读取风扇曲线（只读，约 2 秒）…', 'busy');
  try {
    const data = await invoke(IPC.getFanCurves);
    renderCurves(data);
    setStatus('风扇曲线已读取（只读）', 'ok');
  } catch (e) {
    setStatus(`读取曲线失败：${e}`, 'err');
  } finally {
    setBusy(false);
  }
}

// ---------- 事件绑定 ----------

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
ui.loadCurves.addEventListener('click', doLoadCurves);

ui.relaunch.addEventListener('click', async () => {
  setBusy(true);
  setStatus('正在请求管理员权限…', 'busy');
  try {
    await invoke(IPC.relaunchElevated);
  } catch (e) {
    setStatus(`提权失败：${e}`, 'err');
    setBusy(false);
  }
});

// ---------- 启动 ----------

function tickClock() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  ui.clock.textContent = `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function boot() {
  const missing = findMissingElements(ui);
  if (missing.length > 0) {
    setStatus(`界面元素缺失: ${missing.join(', ')}`, 'err');
    console.error('[mechrevo-max]', `界面元素缺失: ${missing.join(', ')}`, {
      hint: 'app.js 里的 ui 映射与 index.html 的 id 对不上',
    });
    return;
  }

  for (const t of ui.powerTabs.querySelectorAll('.tab')) {
    t.setAttribute('aria-selected', t.dataset.powerMode === activePowerMode ? 'true' : 'false');
  }
  syncSliderOutputs();
  renderSampleInfo();

  refresh();
  startPolling();
  loadProfiles();
  setInterval(tickClock, 1000);
  tickClock();
}

document.addEventListener('DOMContentLoaded', boot);
