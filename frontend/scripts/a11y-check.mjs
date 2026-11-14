// Usage: node scripts/a11y-check.mjs <baseUrl> [routes...]   (routes default to "/")
// Runs axe on each route in headless Chromium, prints violations grouped by impact, exits 1 on serious/critical.
import { chromium } from 'playwright';
import AxeBuilder from '@axe-core/playwright';

const [base, ...rest] = process.argv.slice(2);
if (!base) {
  console.error('usage: node scripts/a11y-check.mjs <baseUrl> [routes...]');
  process.exit(2);
}
const routes = rest.length ? rest : ['/'];
const order = ['critical', 'serious', 'moderate', 'minor'];

const browser = await chromium.launch();
let failed = false;
try {
  for (const route of routes) {
    const page = await (await browser.newContext()).newPage();
    await page.goto(new URL(route, base).href, { waitUntil: 'load' });
    await page.waitForTimeout(2000); // let client-rendered content mount; networkidle never fires under next dev (HMR socket)
    const { violations } = await new AxeBuilder({ page }).analyze();
    console.log(`\n== ${route}: ${violations.length} violation(s)`);
    for (const impact of order) {
      for (const v of violations.filter((x) => x.impact === impact)) {
        console.log(`[${impact}] ${v.id}: ${v.help} (${v.nodes.length} node(s))`);
        for (const n of v.nodes.slice(0, 3)) console.log(`    ${n.target.join(' ')}`);
        if (impact === 'critical' || impact === 'serious') failed = true;
      }
    }
    await page.context().close();
  }
} finally {
  await browser.close();
}
process.exit(failed ? 1 : 0);
