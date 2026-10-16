// 通过 WebView2 的 CDP 接口查询页面真实布局数据.
//
// 用途: 无需截图即可获得视口尺寸、元素实际宽高、计算样式,
//       用于诊断 CSS 布局问题并作为 UI 回归验证工具.
//
// 前置: 以 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 启动应用
//
// 用法: node scripts/cdp_probe.mjs [表达式]

const PORT = process.env.CDP_PORT || 9222;

const DEFAULT_EXPR = `JSON.stringify({
  inner: [window.innerWidth, window.innerHeight],
  dpr: window.devicePixelRatio,
  body: [document.body.clientWidth, document.body.scrollWidth],
  shell: (() => { const e = document.querySelector('.shell'); const r = e.getBoundingClientRect();
    return [Math.round(r.width), e.scrollWidth]; })(),
  stage: (() => { const e = document.querySelector('.stage'); const r = e.getBoundingClientRect();
    return [Math.round(r.width), e.scrollWidth]; })(),
  stageMain: (() => { const e = document.querySelector('.stage-main'); const r = e.getBoundingClientRect();
    return [Math.round(r.width), e.scrollWidth]; })(),
  readouts: (() => { const e = document.querySelector('.readouts');
    return [Math.round(e.getBoundingClientRect().width), e.scrollWidth, e.children.length,
      getComputedStyle(e).display, getComputedStyle(e).flexWrap]; })(),
  cardWidths: [...document.querySelectorAll('.readout')].map(c => Math.round(c.getBoundingClientRect().width)),
  overflowX: document.documentElement.scrollWidth - window.innerWidth,
})`;

async function main() {
  const expr = process.argv[2] || DEFAULT_EXPR;

  const listRes = await fetch(`http://127.0.0.1:${PORT}/json`);
  const targets = await listRes.json();
  const page = targets.find((t) => t.type === 'page');
  if (!page) {
    console.error('未找到页面目标. 可用目标:', targets.map((t) => `${t.type}:${t.title}`).join(', '));
    process.exit(1);
  }

  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    ws.onopen = res;
    ws.onerror = (e) => rej(new Error(`WebSocket 连接失败: ${e.message || e}`));
  });

  const result = await new Promise((res, rej) => {
    const id = 1;
    const timer = setTimeout(() => rej(new Error('CDP 请求超时')), 10000);
    ws.onmessage = (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id === id) {
        clearTimeout(timer);
        res(msg);
      }
    };
    ws.send(JSON.stringify({
      id,
      method: 'Runtime.evaluate',
      params: { expression: expr, returnByValue: true, awaitPromise: true },
    }));
  });

  ws.close();

  const r = result.result;
  if (r.exceptionDetails) {
    console.error('页面执行异常:', JSON.stringify(r.exceptionDetails, null, 2));
    process.exit(2);
  }

  const value = r.result.value;
  try {
    console.log(JSON.stringify(JSON.parse(value), null, 2));
  } catch {
    console.log(value);
  }
}

main().catch((e) => {
  console.error('错误:', e.message);
  process.exit(1);
});
