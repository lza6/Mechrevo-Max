// 机械革命控制台 · 前端逻辑
// 与 Rust 后端通过 Tauri IPC 通信

const { invoke } = window.__TAURI__.core;

/**
 * IPC 契约表 —— **后端命令名的唯一来源**。
 *
 * 为什么要集中：`ipc_contract.rs` 会在编译期解析本表，与后端注册的命令集
 * 做双向比对。命令名散落在各处时，改一个名字只能靠人记得同步，测试抓不到。
 * 因此这里约定：**任何 invoke 都必须经由本表**（测试会检查源码里没有直接写
 * 字符串字面量作为命令名的调用）。
 *
 * 修改本表后，必须同步 `app/src-tauri/tests/ipc_contract.rs`。
 */
const IPC = {
  getStatus: 'get_status',
  setMode: 'set_mode',
  setPowerLimits: 'set_power_limits',
  undoPower: 'undo_power',
  undoMode: 'undo_mode',
  relaunchElevated: 'relaunch_elevated',
};

const POLL_MS = 2000;
const MAX_RPM_SCALE = 6000; // 进度条量程

const el = (id) => document.getElementById(id);

/**
 * 启动自检：收集 XML/HTML 里 id 与 JS 期望不一致的元素。
 *
 * 避免"某个 UI 元素不存在 → 首屏就 `null.textContent` 抛异常 → 整个界面卡死"
 * 这种最难排查的故障。启动时一次性报清楚缺了什么。
 *
 * @param {Record<string, HTMLElement|null>} map
 * @returns {string[]} 缺失元素的名称列表
 */
function findMissingElements(map) {
  return Object.entries(map)
    .filter(([, node]) => !node)
    .map(([name]) => name);
}

const ui = {
  ecPill: el('ec-pill'),
  ecText: el('ec-text'),
  projectId: el('project-id'),
  alert: el('ec-alert'),
  alertTitle: el('ec-alert-title'),
  alertMsg: el('ec-alert-msg'),
  alertDriver: el('alert-driver'),
  relaunch: el('relaunch'),
  stage: el('stage'),
  modeList: el('mode-list'),
  modeRaw: el('mode-raw'),
  undoMode: el('undo-mode'),
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
  statusMsg: el('status-msg'),
  persistError: el('persist-error'),
  alertLive: el('alert-live'),
  clock: el('clock'),
  cpuTemp: el('readout-cpu-temp'),
  gpuTemp: el('readout-gpu-temp'),
  cpu: el('readout-cpu'),
  gpu: el('readout-gpu'),
  power: el('readout-power'),
};

/** 当前选中的功耗模式 tab */
let activePowerMode = 'gaming';
/** 最后一次从硬件读回的功耗值，用于"重置"与脏检查 */
let baseline = { gaming: null, office: null };
/** EC 是否可用 */
let ecReady = false;
/** 轮询定时器 */
let pollTimer = null;
/** 是否已有一次 refresh 在飞（防止轮询与动作后刷新重叠） */
let refreshing = false;
/** 在飞期间收到的"强制刷新"请求，等本轮结束后补一次 */
let pendingRefresh = false;
/**
 * 用户是否正在编辑功耗滑块。
 *
 * 这是必需的：轮询每 2 秒会重新从硬件读值并回填滑块，
 * 如果期间用户正拖着滑块，回填会把用户输入冲掉（拖到一半跳回去）。
 * 进入编辑（pointerdown / 键盘聚焦）时置位，编辑结束或应用后复位。
 */
let editingPower = false;

// ---------- 工具 ----------

function setStatus(msg, kind = '') {
  ui.statusMsg.textContent = msg;
  ui.statusMsg.dataset.kind = kind;
}

/**
 * 进入/退出"忙"状态。
 *
 * 忙期间**必须停掉轮询**：EC 硬件不支持并发访问，用户在切档位/写功耗时
 * 若 2 秒轮询同时在读，就会与写入交错（后端虽然串行化了，但让轮询白白排队
 * 只会拖慢操作反馈，也会让状态栏读数在写入过程中乱跳）。
 */
function setBusy(busy) {
  document.body.dataset.busy = busy ? 'true' : 'false';
  if (busy) {
    stopPolling();
  } else {
    startPolling();
  }
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
  return {
    pl1: Number(ui.pl1.value),
    pl2: Number(ui.pl2.value),
    pl4: Number(ui.pl4.value),
  };
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
    const text = `${node.value} W`;
    ui[out].textContent = text;
    // 读屏不要念裸数字：给出单位与语义
    node.setAttribute('aria-valuetext', `${node.value} 瓦，${label}`);
    paintRange(node);
  }
  updateDirty();
}

function updateDirty() {
  const base = baseline[activePowerMode];
  const cur = readSliders();
  const dirty = base
    ? cur.pl1 !== base.pl1 || cur.pl2 !== base.pl2 || cur.pl4 !== base.pl4
    : false;
  ui.pending.textContent = dirty ? '有未应用的修改' : '未修改';
  ui.pending.dataset.dirty = dirty ? 'true' : 'false';
  ui.applyPower.disabled = !ecReady || !dirty;
}

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

  // 权限不足 / 内部状态异常 → 一键重启；驱动未就绪 → 给出命令行步骤
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
    const active = btn.dataset.mode === mode;
    btn.setAttribute('aria-pressed', active ? 'true' : 'false');
  }
  ui.modeList.querySelectorAll('.mode-btn').forEach((b) => {
    b.disabled = !ecReady;
  });
}

function renderReadout(node, rpm) {
  const valueEl = node.querySelector('[data-role="value"]');
  const barEl = node.querySelector('[data-role="bar"]');
  const stateEl = node.querySelector('[data-role="state"]');

  if (rpm === null || rpm === undefined) {
    valueEl.textContent = '—';
    barEl.style.width = '0%';
    stateEl.textContent = '无数据';
    node.classList.remove('is-hot');
    return;
  }

  valueEl.textContent = String(rpm);
  const pct = Math.min(100, (rpm / MAX_RPM_SCALE) * 100);
  barEl.style.width = `${pct}%`;

  const hot = rpm >= 4500;
  node.classList.toggle('is-hot', hot);
  stateEl.textContent = rpm === 0 ? '静止' : hot ? '高转速' : '运转中';
}

/**
 * 渲染温度卡。
 *
 * @param {HTMLElement} node 卡片元素
 * @param {number|null|undefined} celsius 温度（°C），无数据传 null
 * @param {{warm:number, hot:number}} threshold 判定阈值
 */
function renderTempReadout(node, celsius, threshold) {
  const valueEl = node.querySelector('[data-role="value"]');
  const stateEl = node.querySelector('[data-role="state"]');
  const noteEl = node.querySelector('[data-role="note"]');

  if (celsius === null || celsius === undefined) {
    valueEl.textContent = '—';
    stateEl.textContent = '无数据';
    noteEl.textContent = '—';
    node.classList.remove('is-hot');
    node.classList.remove('is-warm');
    return;
  }

  valueEl.textContent = String(celsius);
  const hot = celsius >= threshold.hot;
  const warm = !hot && celsius >= threshold.warm;
  node.classList.toggle('is-hot', hot);
  node.classList.toggle('is-warm', warm);
  stateEl.textContent = hot ? '过热' : warm ? '偏高' : '正常';
  noteEl.textContent = hot ? '建议降低负载或切到更强散热' : warm ? '温度偏高，注意散热' : '温度正常';
}

function renderPowerReadout(isAc, source) {
  const valueEl = ui.power.querySelector('[data-role="value"]');
  const noteEl = ui.power.querySelector('[data-role="note"]');
  if (isAc === null || isAc === undefined) {
    valueEl.textContent = '—';
    noteEl.textContent = '—';
    return;
  }
  valueEl.textContent = isAc ? '适配器' : '电池';
  noteEl.textContent = `编码 ${fmt(source)}`;
}

function renderStatus(s) {
  renderEc(s.ec);
  ui.projectId.textContent = fmt(s.project_id);
  renderMode(s.mode);
  renderTempReadout(ui.cpuTemp, s.cpu_temp_c, { warm: 60, hot: 80 });
  renderTempReadout(ui.gpuTemp, s.gpu_temp_c, { warm: 60, hot: 80 });
  renderReadout(ui.cpu, s.cpu_fan_rpm);
  renderReadout(ui.gpu, s.gpu_fan_rpm);
  renderPowerReadout(s.is_ac_powered, s.power_source);

  if (s.mode_raw) {
    const [a, b] = s.mode_raw;
    ui.modeRaw.textContent =
      `751=0x${a.toString(16).toUpperCase().padStart(2, '0')} · ` +
      `7AB=0x${b.toString(16).toUpperCase().padStart(2, '0')}`;
    // 读屏念 "七五一等于零 x 零零" 没有意义，给一句人话
    ui.modeRaw.setAttribute('aria-label', `档位原始寄存器：751 为 ${a}，7AB 为 ${b}`);
  } else {
    ui.modeRaw.textContent = '751=— · 7AB=—';
    ui.modeRaw.setAttribute('aria-label', '档位原始寄存器：无数据');
  }

  // 上一次失败要留在界面上（轮询会不断刷状态栏，历史错误不能跟着消失）
  if (s.last_error) {
    ui.persistError.hidden = false;
    ui.persistError.textContent = `上次操作失败：${s.last_error}`;
    ui.persistError.title = s.last_error;
  } else {
    ui.persistError.hidden = true;
    ui.persistError.textContent = '';
    ui.persistError.title = '';
  }

  baseline.gaming = s.gaming_power;
  baseline.office = s.office_power;

  // 只有在用户**没有**正在编辑滑块时才回填。
  // 否则每 2 秒的轮询会把用户拖到一半的值冲掉。
  const dirty = ui.pending.dataset.dirty === 'true';
  if (!editingPower && !dirty) {
    setSliders(baseline[activePowerMode]);
  }
  updateDirty();

  ui.undoMode.disabled = !ecReady || !s.can_undo_mode;
  ui.undoPower.disabled = !ecReady || !s.can_undo_power;
}

// ---------- 行为 ----------

/**
 * 拉取一次状态。
 *
 * @param {boolean} force 忙碌结束后要强制刷新时传 true —— 若此刻正好有一轮
 *   在飞，会记下待刷新标记，等它回来再补一次，避免界面停在上一次动作前的值。
 */
async function refresh(force = false) {
  if (refreshing) {
    if (force) pendingRefresh = true;
    return;
  }
  refreshing = true;
  try {
    const s = await invoke(IPC.getStatus);
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
    const msg = await invoke(IPC.setMode, { mode });
    setStatus(msg, 'ok');
    await refresh(true);
  } catch (e) {
    setStatus(`切换失败：${e}`, 'err');
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
    // 应用成功后释放编辑锁，让轮询恢复回填（现在回填的是新值）
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

async function doUndo(which) {
  setBusy(true);
  setStatus('正在撤销…', 'busy');
  try {
    const msg = await invoke(which === 'mode' ? IPC.undoMode : IPC.undoPower);
    setStatus(msg, 'ok');
    await refresh(true);
  } catch (e) {
    setStatus(`撤销失败：${e}`, 'err');
    await refresh(true);
  } finally {
    setBusy(false);
  }
}

// ---------- 事件绑定 ----------

ui.modeList.addEventListener('click', (ev) => {
  const btn = ev.target.closest('.mode-btn');
  if (btn && ecReady) doSwitchMode(btn.dataset.mode);
});

ui.powerTabs.addEventListener('click', (ev) => {
  const tab = ev.target.closest('.tab');
  if (!tab) return;
  const nextMode = tab.dataset.powerMode;
  if (nextMode === activePowerMode) return;

  // 切 tab 会丢弃未保存的改动 —— 先提示，避免用户静默丢失输入
  if (ui.pending.dataset.dirty === 'true') {
    const ok = window.confirm(
      '当前模式有未应用的功耗改动。\n切换将丢弃这些改动，确定继续吗？'
    );
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

// 滑块编辑锁：防止 2 秒轮询在用户拖动时把值冲掉。
// pointerdown/pointerup 覆盖鼠标与触摸；focus/blur 覆盖键盘操作。
[ui.pl1, ui.pl2, ui.pl4].forEach((input) => {
  input.addEventListener('input', syncSliderOutputs);

  input.addEventListener('pointerdown', () => {
    editingPower = true;
  });
  input.addEventListener('pointerup', () => {
    editingPower = false;
  });
  input.addEventListener('pointercancel', () => {
    editingPower = false;
  });
  input.addEventListener('focus', () => {
    editingPower = true;
  });
  input.addEventListener('blur', () => {
    editingPower = false;
  });

  // 键盘方向键调整也应视为编辑中
  input.addEventListener('keydown', () => {
    editingPower = true;
  });

  // 防止拖动过程中页面滚动/选中文本
  input.addEventListener('pointermove', (ev) => {
    if (editingPower) ev.preventDefault();
  });
});

// 窗口失去焦点时务必解锁，否则轮询会永久停止回填
window.addEventListener('blur', () => {
  editingPower = false;
});

ui.applyPower.addEventListener('click', doApplyPower);
ui.undoPower.addEventListener('click', () => doUndo('power'));
ui.undoMode.addEventListener('click', () => doUndo('mode'));
ui.resetPower.addEventListener('click', () => {
  editingPower = false;
  setSliders(baseline[activePowerMode]);
  setStatus('已重置为读回值');
});

ui.relaunch.addEventListener('click', async () => {
  setBusy(true);
  setStatus('正在请求管理员权限…', 'busy');
  try {
    await invoke(IPC.relaunchElevated);
    // 提权实例启动后当前进程会自行退出，一般不会走到这里
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
  // 启动自检：id 与 HTML 不一致时立刻报清楚，而不是首屏抛异常
  const missing = findMissingElements(ui);
  if (missing.length > 0) {
    const msg = `界面元素缺失: ${missing.join(', ')}`;
    setStatus(msg, 'err');
    // 同时打印到控制台便于排障
    console.error('[mechrevo-max]', msg, {
      hint: 'app.js 里的 ui 映射与 index.html 的 id 对不上',
    });
    return;
  }

  // 默认选中"游戏"功耗 tab
  for (const t of ui.powerTabs.querySelectorAll('.tab')) {
    t.setAttribute('aria-selected', t.dataset.powerMode === activePowerMode ? 'true' : 'false');
  }
  syncSliderOutputs();

  refresh();
  startPolling();
  setInterval(tickClock, 1000);
  tickClock();
}

document.addEventListener('DOMContentLoaded', boot);
