// Verification sweep for the redefined scene.fill_level (literal tank-fill %).
// Drives the DEFAULT scene (preset 0, falling-blob = resting floor slab) at
// fill_level = 10/20/50/100% in one Chrome session, applying each via
// set_setting + reset, warming up, then reading stats_json
// (liquid_cells / filled_volume / liquid_fraction) and screenshotting the canvas.
//
// Run from Windows node (real-GPU Chrome), pointed at the WSL dev server:
//   node tools/fill_sweep.mjs
// Screenshots + a JSON report land in the repo captures/ dir.

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const CAPTURES_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../../captures");
const URL = process.env.URL || "http://localhost:5184/";
const CHROME =
  process.env.CHROME || "C:/Program Files/Google/Chrome/Application/chrome.exe";
const WARMUP_MS = parseInt(process.env.WARMUP_MS || "4000", 10);

// fill is a 0-100 percentage now. Default scene = preset 0 (falling-blob slab).
const CASES = [
  { name: "fill_10", preset: 0, fill: 10, density: 8 },
  { name: "fill_20", preset: 0, fill: 20, density: 8 },
  { name: "fill_50", preset: 0, fill: 50, density: 8 },
  { name: "fill_100", preset: 0, fill: 100, density: 8 },
];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

mkdirSync(CAPTURES_DIR, { recursive: true });
const { default: puppeteer } = await import("puppeteer-core");
const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: "new",
  args: [
    "--enable-unsafe-webgpu",
    "--enable-features=Vulkan",
    "--use-angle=default",
    "--no-sandbox",
    "--window-size=1280,800",
  ],
});

const report = [];
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
  page.on("pageerror", (e) => console.log("[pageerror] " + e.message));
  await page.goto(URL, { waitUntil: "networkidle2", timeout: 30000 });
  await sleep(2500);

  for (const c of CASES) {
    const applied = await page.evaluate((cfg) => {
      const f = window.__fluid;
      f.set_setting("scene.preset", cfg.preset);
      f.set_setting("grid.res_x", 64);
      f.set_setting("grid.res_y", 64);
      f.set_setting("grid.res_z", 64);
      f.set_setting("particles.count", 0); // Auto so density drives the count
      f.set_setting("scene.fill_level", cfg.fill);
      f.set_setting("particles.density", cfg.density);
      const ok = f.reset();
      return { ok };
    }, c);

    // Sample early (near-seeded, before the slab settles) and settled.
    let early = null;
    const start = Date.now();
    while (Date.now() - start < WARMUP_MS) {
      const s = await page.evaluate(() =>
        window.__fluid ? JSON.parse(window.__fluid.stats_json()) : null,
      );
      const lc = s?.gpu?.liquid_cells;
      if (early == null && Number.isFinite(lc) && Date.now() - start >= 800) early = lc;
      await sleep(300);
    }
    const stats = await page.evaluate(() =>
      window.__fluid ? JSON.parse(window.__fluid.stats_json()) : null,
    );
    const out = join(CAPTURES_DIR, c.name + ".png");
    await page.screenshot({ path: out });
    const row = {
      ...c,
      reset_ok: applied.ok,
      liquid_cells: stats?.gpu?.liquid_cells ?? null,
      liquid_cells_early: early,
      filled_volume: stats?.filled_volume ?? null,
      liquid_fraction: stats?.liquid_fraction ?? null,
      requested_particles: stats?.requested_particles ?? null,
      seeded_particles: stats?.particles ?? null,
      png: out,
    };
    report.push(row);
    console.log("[fill_sweep] " + JSON.stringify(row));
  }

  writeFileSync(
    join(CAPTURES_DIR, "fill_sweep.report.json"),
    JSON.stringify({ url: URL, cases: report }, null, 2) + "\n",
  );
  console.log("[fill_sweep] wrote captures/fill_sweep.report.json");
} finally {
  await browser.close();
}
