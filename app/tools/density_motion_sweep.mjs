// Verification sweep for the anti-clump rest_density coupling.
// Varies the ACTUAL particle-density knob (particles.density ∈ {1,8,32}) with a
// FIXED scene + FIXED camera (no rotation) + equal warm-up, and measures the
// visible water volume two ways at t≈1s, 4s, 8s:
//   - liquid_cells (gpu stats) — the pressure-region cell count
//   - water_pixels — non-black water coverage in the canvas
// Auto-roll is forced OFF (manual mode) so the box never rocks during measurement.
//
// Run from Windows node:
//   cd app/tools && cmd.exe /c 'pushd \\wsl.localhost\Ubuntu-24.04\home\adamg\fluid-simulation\app\tools && node density_motion_sweep.mjs & popd'

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const CAPTURES_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../../captures");
const URL = process.env.URL || "http://localhost:5184/";
const CHROME =
  process.env.CHROME || "C:/Program Files/Google/Chrome/Application/chrome.exe";

const DENSITIES = [1, 8, 32];
const SAMPLE_TIMES_MS = [1000, 4000, 8000];
const TOTAL_MS = 8600;

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

// Count non-near-black pixels in the canvas (a water/coverage proxy). Runs in-page.
const COUNT_WATER_PIXELS = () => {
  const cv = document.querySelector("canvas");
  if (!cv) return null;
  const off = document.createElement("canvas");
  off.width = cv.width;
  off.height = cv.height;
  const ctx = off.getContext("2d");
  ctx.drawImage(cv, 0, 0);
  const { data } = ctx.getImageData(0, 0, off.width, off.height);
  let water = 0;
  // "Water" = pixels meaningfully brighter than the dark backdrop. Blue-biased so
  // the gradient backdrop isn't fully counted; threshold tuned to the hero look.
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i], g = data[i + 1], b = data[i + 2];
    const lum = 0.299 * r + 0.587 * g + 0.114 * b;
    if (lum > 40 && b > 50 && b >= r) water++;
  }
  return { water, total: off.width * off.height };
};

const report = [];
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
  page.on("pageerror", (e) => console.log("[pageerror] " + e.message));
  await page.goto(URL, { waitUntil: "networkidle2", timeout: 30000 });
  await sleep(2500);

  for (const density of DENSITIES) {
    const applied = await page.evaluate((d) => {
      const f = window.__fluid;
      // FIXED scene: default falling-blob, 64^3, 20% fill, Auto count.
      f.set_setting("scene.preset", 0);
      f.set_setting("grid.res_x", 64);
      f.set_setting("grid.res_y", 64);
      f.set_setting("grid.res_z", 64);
      f.set_setting("scene.fill_level", 20);
      f.set_setting("particles.count", 0);
      f.set_setting("particles.density", d);
      // Manual mode equivalents: no tank roll, no waves. rest_density stays 0 (Auto).
      f.set_setting("interaction.auto_roll_enabled", 0);
      f.set_setting("interaction.wave_enabled", 0);
      f.set_setting("physics.rest_density", 0);
      // FIXED camera (registry defaults; no rotation thereafter).
      f.set_setting("camera.rot_x", -0.2);
      f.set_setting("camera.rot_y", 0.6);
      f.set_setting("camera.rot_z", 0.0);
      const ok = f.reset();
      const s = JSON.parse(f.stats_json());
      return { ok, seeded: s?.particles ?? null, rest: s?.gpu?.rest_density ?? null };
    }, density);

    const start = Date.now();
    const series = [];
    let nextIdx = 0;
    while (Date.now() - start < TOTAL_MS) {
      const t = Date.now() - start;
      if (nextIdx < SAMPLE_TIMES_MS.length && t >= SAMPLE_TIMES_MS[nextIdx]) {
        const stamp = SAMPLE_TIMES_MS[nextIdx];
        const stats = await page.evaluate(
          () => (window.__fluid ? JSON.parse(window.__fluid.stats_json()) : null),
        );
        const px = await page.evaluate(COUNT_WATER_PIXELS);
        series.push({
          t_ms: stamp,
          liquid_cells: stats?.gpu?.liquid_cells ?? null,
          water_pixels: px?.water ?? null,
        });
        const out = join(CAPTURES_DIR, `dms_d${density}_t${stamp}.png`);
        await page.screenshot({ path: out });
        nextIdx++;
      }
      await sleep(150);
    }
    const row = { density, reset_ok: applied.ok, seeded: applied.seeded, series };
    report.push(row);
    console.log("[dms] " + JSON.stringify(row));
  }

  // Per-time invariance vs density 8 (the reference look).
  const at = (d, t, key) =>
    report.find((r) => r.density === d)?.series.find((s) => s.t_ms === t)?.[key];
  const summary = {};
  for (const t of SAMPLE_TIMES_MS) {
    for (const key of ["liquid_cells", "water_pixels"]) {
      const ref = at(8, t, key);
      const d1 = at(1, t, key);
      const d32 = at(32, t, key);
      summary[`${key}_t${t}`] = {
        d1, d8: ref, d32,
        d1_over_d8: ref ? d1 / ref : null,
        d32_over_d8: ref ? d32 / ref : null,
      };
    }
  }
  console.log("[dms] SUMMARY " + JSON.stringify(summary, null, 2));
  writeFileSync(
    join(CAPTURES_DIR, "density_motion_sweep.report.json"),
    JSON.stringify({ url: URL, cases: report, summary }, null, 2) + "\n",
  );
} finally {
  await browser.close();
}
