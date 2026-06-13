// One-off Phase-1 calibration sweep for the volume/density decoupling plan.
// Drives the real-GPU page through (preset, fill_level, density) cases in a single
// Chrome session, applying each via set_setting + reset, warming up, then reading
// stats_json (liquid_cells / filled_volume) and screenshotting the canvas.
//
// Run from Windows node via the same cmd.exe/pushd wrapper as capture.mjs:
//   pushd \\wsl.localhost\...\app\tools && node vdd_sweep.mjs
// Screenshots + a JSON report land in the repo captures/ dir.

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const CAPTURES_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../../captures");
const URL = process.env.URL || "http://localhost:5184/";
const CHROME =
  process.env.CHROME || "C:/Program Files/Google/Chrome/Application/chrome.exe";
const WARMUP_MS = parseInt(process.env.WARMUP_MS || "9000", 10);

// preset: 0 falling-blob, 1 dam-break, 2 double-splash.
const CASES = [
  { name: "vdd_water_low", preset: 1, fill: 0.3, density: 8 },
  { name: "vdd_water_high", preset: 1, fill: 0.9, density: 8 },
  { name: "vdd_density_8", preset: 1, fill: 0.75, density: 8 },
  { name: "vdd_density_2", preset: 1, fill: 0.75, density: 2 },
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

    // Sample liquid_cells over the warmup so we can report both an early
    // (near-seeded) value and the settled value — dam-break collapses, so the
    // density-invariance of the *seeded body* is clearest early, while the late
    // sample shows the dynamic state.
    const samples = [];
    const start = Date.now();
    let early = null;
    while (Date.now() - start < WARMUP_MS) {
      const s = await page.evaluate(() =>
        window.__fluid ? JSON.parse(window.__fluid.stats_json()) : null,
      );
      const lc = s?.gpu?.liquid_cells;
      const t = Date.now() - start;
      if (Number.isFinite(lc)) {
        samples.push(lc);
        if (early == null && t >= 1200) early = lc;
      }
      await sleep(300);
    }
    const stats = await page.evaluate(() =>
      window.__fluid ? JSON.parse(window.__fluid.stats_json()) : null,
    );
    const maxLc = samples.length ? Math.max(...samples) : null;
    const out = join(CAPTURES_DIR, c.name + ".png");
    await page.screenshot({ path: out });
    const row = {
      ...c,
      reset_ok: applied.ok,
      liquid_cells: stats?.gpu?.liquid_cells ?? null,
      liquid_cells_early: early,
      liquid_cells_max: maxLc,
      filled_volume: stats?.filled_volume ?? null,
      liquid_fraction: stats?.liquid_fraction ?? null,
      requested_particles: stats?.requested_particles ?? null,
      seeded_particles: stats?.particles ?? null,
      png: out,
    };
    report.push(row);
    console.log("[sweep] " + JSON.stringify(row));
  }

  // Invariance summaries.
  const byName = Object.fromEntries(report.map((r) => [r.name, r]));
  const water =
    byName.vdd_water_high.liquid_cells / byName.vdd_water_low.liquid_cells;
  const dens =
    byName.vdd_density_8.liquid_cells / byName.vdd_density_2.liquid_cells;
  const densEarly =
    byName.vdd_density_8.liquid_cells_early /
    byName.vdd_density_2.liquid_cells_early;
  const densMax =
    byName.vdd_density_8.liquid_cells_max /
    byName.vdd_density_2.liquid_cells_max;
  const summary = {
    waterline_high_over_low_liquid_cells: water,
    density8_over_density2_liquid_cells_settled: dens,
    density8_over_density2_liquid_cells_early: densEarly,
    density8_over_density2_liquid_cells_max: densMax,
    density_invariance_early_within_15pct: Math.abs(densEarly - 1) <= 0.15,
  };
  console.log("[sweep] SUMMARY " + JSON.stringify(summary));
  writeFileSync(
    join(CAPTURES_DIR, "vdd_sweep.report.json"),
    JSON.stringify({ url: URL, cases: report, summary }, null, 2) + "\n",
  );
} finally {
  await browser.close();
}
