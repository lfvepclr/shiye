// 分享页实测：叠加对齐、文字溢出、模糊/高清切换、localStorage 恢复、LCP 与图片字节。
// 依赖 playwright-core：npm i playwright-core（或用 PLAYWRIGHT_PATH 指向已安装位置）
const { chromium } = require(process.env.PLAYWRIGHT_PATH || 'playwright-core');

const URL = process.argv[2];
const WIDTHS = (process.env.WIDTHS || '390,1080').split(',').map(Number);

(async () => {
  const browser = await chromium.launch({ channel: 'chrome' });
  for (const width of WIDTHS) {
    const ctx = await browser.newContext({ viewport: { width, height: 900 } });
    const page = await ctx.newPage();
    await page.addInitScript(() => {
      window.__lcp = 0;
      new PerformanceObserver((list) => {
        for (const e of list.getEntries()) { window.__lcp = e.startTime; }
      }).observe({ type: 'largest-contentful-paint', buffered: true });
    });

    const navStart = Date.now();
    await page.goto(URL, { waitUntil: 'load' });
    await page.waitForSelector('.page-card');
    await page.waitForTimeout(500);
    const loadMs = Date.now() - navStart;

    const first = await page.evaluate(async () => {
      const data = await (await fetch('./state')).json();
      const cards = Array.from(document.querySelectorAll('.page-card'));
      let blocks = 0, maxPx = 0, sumPx = 0, clipped = 0, outOfCard = 0, clippedSamples = [];
      cards.forEach((card, i) => {
        const pg = data.pages[i];
        const cr = card.getBoundingClientRect();
        (data.blocks[pg.id] || []).forEach((b) => {
          const el = card.querySelector('[data-key="' + pg.id + ':' + b.ord + '"]');
          if (!el) return;
          const r = el.getBoundingClientRect();
          const x1 = ((r.left - cr.left) / cr.width) * pg.w;
          const y1 = ((r.top - cr.top) / cr.height) * pg.h;
          const err = Math.max(Math.abs(x1 - b.x1), Math.abs(y1 - b.y1));
          blocks++; sumPx += err;
          if (err > maxPx) maxPx = err;
          if (el.scrollHeight > el.clientHeight + 2) {
            clipped++;
            if (clippedSamples.length < 3) {
              clippedSamples.push({ over: el.scrollHeight - el.clientHeight, boxH: Math.round(r.height) });
            }
          }
          if (r.left < cr.left - 1 || r.right > cr.right + 1 || r.top < cr.top - 1 || r.bottom > cr.bottom + 1) outOfCard++;
        });
      });
      const imgs = cards.map((c) => c.querySelector('.page-img').currentSrc);
      const res = performance.getEntriesByType('resource').filter((r) => r.name.indexOf('/img/') >= 0);
      return {
        pages: cards.length,
        blocks,
        maxErrPx: +maxPx.toFixed(2),
        avgErrPx: +(sumPx / blocks).toFixed(3),
        maxErrPct: +((maxPx / data.pages[0].w) * 100).toFixed(3),
        clipped,
        clippedSamples,
        outOfCard,
        thumbCount: imgs.filter((u) => u.indexOf('v=thumb') >= 0).length,
        dispCount: imgs.filter((u) => u.indexOf('v=disp') >= 0).length,
        thumbBytes: res.reduce((s, r) => s + (r.transferSize || r.decodedBodySize || 0), 0),
        lcpMs: Math.round(window.__lcp || 0),
        fontPx: parseFloat(getComputedStyle(cards[0].querySelector('.ovl')).fontSize),
        cardW: Math.round(cards[0].getBoundingClientRect().width),
      };
    });

    const before = await page.evaluate(() => performance.getEntriesByType('resource').length);
    await page.click('#btn-hd');
    await page.waitForTimeout(800);
    const hd = await page.evaluate((n) => {
      const imgs = Array.from(document.querySelectorAll('.page-img')).map((i) => i.currentSrc);
      const res = performance.getEntriesByType('resource').slice(n).filter((r) => r.name.indexOf('/img/') >= 0);
      return {
        btn: document.getElementById('btn-hd').textContent,
        hiRes: localStorage.getItem('shiye.hiRes'),
        dispCount: imgs.filter((u) => u.indexOf('v=disp') >= 0).length,
        thumbCount: imgs.filter((u) => u.indexOf('v=thumb') >= 0).length,
        dispBytes: res.reduce((s, r) => s + (r.transferSize || r.decodedBodySize || 0), 0),
      };
    }, before);

    await page.reload({ waitUntil: 'load' });
    await page.waitForSelector('.page-card');
    await page.waitForTimeout(400);
    const after = await page.evaluate(() => {
      const imgs = Array.from(document.querySelectorAll('.page-img')).map((i) => i.currentSrc);
      return {
        btn: document.getElementById('btn-hd').textContent,
        dispCount: imgs.filter((u) => u.indexOf('v=disp') >= 0).length,
      };
    });

    console.log(JSON.stringify({ width, loadMs, ...first, hd, after }));
    await ctx.close();
  }
  await browser.close();
})();
