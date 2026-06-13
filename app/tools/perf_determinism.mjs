// Determinism + perf harness for the workgroup-local pre-accumulation scatter.
//
// Runs Windows-side Chrome (real GPU) via puppeteer-core against the WSL dev
// server (localhost:5184). Two phases:
//
//  DETERMINISM (debug_view=10 "Nearest Z" depth, order-independent per-pixel min):
//    For each config (sort off / sort on) it pauses, applies settings, resets,
//    steps K fixed substeps, screenshots the canvas. The two PNGs must be
//    byte-identical (0-pixel-diff) for bit-identical state.
//
//  PERF (detailed gpu profiling): for each particle count and sort config it
//    resets, runs free for a settle+measure window, and reports the detailed
//    sections scatter / g2p / sort plus sim_ms and fps.
//
// Usage (from Windows node, cwd app/tools):
//   node perf_determinism.mjs det     # determinism only
//   node perf_determinism.mjs perf    # perf sweep only
//   node perf_determinism.mjs all     # both
import { writeFileSync, readFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = resolve(HERE, "../../captures/perf_det");
mkdirSync(OUT, { recursive: true });

const URL = process.env.URL || "http://localhost:5184/";
const CHROME = process.env.CHROME || "C:/Program Files/Google/Chrome/Application/chrome.exe";
const MODE = process.argv[2] || "all";
const STEPS = parseInt(process.env.STEPS || "120", 10);   // fixed substeps for determinism
const SETTLE = parseInt(process.env.SETTLE || "6000", 10);
const MEASURE = parseInt(process.env.MEASURE || "9000", 10);
const GRID = parseInt(process.env.GRID || "128", 10);

// Particle-count targets (overridden via particles.count). Grid capped at 128.
const COUNTS = (process.env.COUNTS || "6600000,13400000,21600000")
  .split(",").map((s) => parseInt(s, 10));
const CADENCES = (process.env.CADENCES || "1,2,4,8").split(",").map((s) => parseInt(s, 10));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function readStats(page) {
  return await page.evaluate(() =>
    window.__fluid ? JSON.parse(window.__fluid.stats_json()) : null);
}

// Apply a settings map then reset; returns the seeded particle count.
async function applyReset(page, settings) {
  return await page.evaluate((s) => {
    const f = window.__fluid;
    if (!f) throw new Error("no __fluid");
    for (const [k, v] of Object.entries(s)) f.set_setting(k, v);
    const ok = f.reset();
    if (!ok) throw new Error("reset rejected for " + JSON.stringify(s));
    const st = JSON.parse(f.stats_json());
    return st?.particles ?? null;
  }, settings);
}

async function pauseAndStep(page, k) {
  // Pause, queue k single-substep ticks. The page's own rAF loop calls frame()
  // each frame and consumes EXACTLY ONE pending step per frame, so draining k
  // steps takes ~k frames. Poll reset_count-stable + wait proportionally.
  await page.evaluate((n) => {
    const f = window.__fluid;
    f.set_paused(true);
    for (let i = 0; i < n; i++) f.step();
  }, k);
  // ~k frames at 60fps, with generous margin for slow frames at high counts.
  await sleep(k * 30 + 1500);
}

async function shoot(page, file) {
  const canvas = await page.$("#gpu-canvas");
  await canvas.screenshot({ path: file });
  return file;
}

// Hash the EXACT rendered pixels. We screenshot the WebGPU canvas to a PNG
// (page.screenshot reliably captures the presented frame), then decode that PNG
// back inside the page via an Image + 2D canvas getImageData and FNV-1a hash the
// pixels. (Directly blitting the WebGPU canvas with drawImage returns blank, so
// we go through the PNG.) Returns {w,h,hash,nonzero}.
async function pixelHash(page) {
  const el = await page.$("#gpu-canvas");
  const b64 = await el.screenshot({ encoding: "base64" });
  return await page.evaluate(async (dataB64) => {
    const img = new Image();
    await new Promise((res, rej) => {
      img.onload = res; img.onerror = rej;
      img.src = "data:image/png;base64," + dataB64;
    });
    const w = img.naturalWidth, h = img.naturalHeight;
    const c2 = document.createElement("canvas");
    c2.width = w; c2.height = h;
    const ctx = c2.getContext("2d", { willReadFrequently: true });
    ctx.drawImage(img, 0, 0);
    const data = ctx.getImageData(0, 0, w, h).data;
    let hash = 2166136261 >>> 0, nz = 0;
    for (let i = 0; i < data.length; i += 4) {
      const r = data[i], g = data[i + 1], b = data[i + 2], a = data[i + 3];
      if (!(r === 0 && g === 0 && b === 0)) nz++;
      hash = (hash ^ r) >>> 0; hash = Math.imul(hash, 16777619) >>> 0;
      hash = (hash ^ g) >>> 0; hash = Math.imul(hash, 16777619) >>> 0;
      hash = (hash ^ b) >>> 0; hash = Math.imul(hash, 16777619) >>> 0;
      hash = (hash ^ a) >>> 0; hash = Math.imul(hash, 16777619) >>> 0;
    }
    return { w, h, hash, nonzero: nz };
  }, b64);
}

function pngEqual(a, b) {
  const ba = readFileSync(a), bb = readFileSync(b);
  if (ba.length !== bb.length) return { equal: false, reason: `size ${ba.length} vs ${bb.length}` };
  for (let i = 0; i < ba.length; i++) if (ba[i] !== bb[i]) return { equal: false, reason: `byte ${i}` };
  return { equal: true };
}

async function determinism(page) {
  console.log(`\n=== DETERMINISM (grid=${GRID}, steps=${STEPS}, debug_view=10) ===`);
  // Modest count so K substeps stay quick; bit-identity is count-independent.
  const base = {
    "grid.res_x": GRID, "grid.res_y": GRID, "grid.res_z": GRID,
    "render.hero.debug_view": 10,
    "particles.count": 0,
    "particles.density": 8,
    "dev.particle_sort_period": 1,
  };
  // Run a labelled config: reset, step K substeps, screenshot (artifact) + hash
  // the exact canvas pixels.
  const run = async (sort, tag) => {
    await applyReset(page, { ...base, "dev.particle_sort": sort });
    await pauseAndStep(page, STEPS);
    await shoot(page, join(OUT, `det_${tag}.png`));
    const h = await pixelHash(page);
    console.log(`  run ${tag}: ${h.w}x${h.h} hash=${h.hash} nonzero=${h.nonzero}`);
    return h;
  };
  // Warm-up run (discarded): the very first reset+step after page load settles
  // pipeline/shader compiles and the initial scene differently; capturing it
  // pollutes the comparison. Throw it away.
  await run(0, "warmup");
  // Self-consistency: OFF vs OFF (validates harness determinism), ON vs ON
  // (sort-path determinism), then the real OFF vs ON+local gate.
  // Interleave OFF/ON so any drift would show up; compare all hashes.
  const off1 = await run(0, "off1");
  const on1 = await run(1, "on1");
  const off2 = await run(0, "off2");
  const on2 = await run(1, "on2");

  const eq = (a, b) => a.w === b.w && a.h === b.h && a.hash === b.hash;
  const c_oo = eq(off1, off2), c_nn = eq(on1, on2);
  const c_on = eq(off1, on1) && eq(off2, on2) && eq(off1, on2);
  console.log(`  OFF vs OFF (harness determinism): ${c_oo ? "MATCH" : "DIFFER"}`);
  console.log(`  ON  vs ON  (sort-path determinism): ${c_nn ? "MATCH" : "DIFFER"}`);
  console.log(`  OFF vs ON+local (THE GATE): ${c_on ? "PASS 0-diff" : "FAIL"}`);
  return c_oo && c_nn && c_on;
}

async function measure(page, settings, label) {
  await applyReset(page, { ...settings, "dev.detailed_gpu_profiling": 1 });
  await page.evaluate(() => window.__fluid.set_paused(false));
  await sleep(SETTLE);
  // Collect stat polls, then keep ONLY GPU-timestamp samples (cpu-wallclock
  // fallback frames are unstable post-reset and must not pollute the medians).
  const all = [];
  const t0 = Date.now();
  while (Date.now() - t0 < MEASURE) {
    const s = await readStats(page);
    if (s) all.push(s);
    await sleep(300);
  }
  const samples = all.filter((s) => s?.timing === "gpu-timestamp" && Number.isFinite(s?.gpu?.sim_ms));
  const med = (arr) => { const a = arr.filter(Number.isFinite).sort((x, y) => x - y); return a.length ? a[Math.floor(a.length / 2)] : null; };
  const pick = (f) => med(samples.map(f));
  const out = {
    label,
    particles: all.at(-1)?.particles ?? null,
    sim_ms: pick((s) => s?.gpu?.sim_ms),
    fps: pick((s) => s?.frame_avg_ms ? 1000 / s.frame_avg_ms : null),
    frame_ms: pick((s) => s?.frame_avg_ms),
    scatter: pick((s) => s?.gpu?.sections?.scatter),
    g2p: pick((s) => s?.gpu?.sections?.g2p),
    sort: pick((s) => s?.gpu?.sections?.sort),
    timing: all.at(-1)?.timing ?? null,
    n_gpu: samples.length, n_total: all.length,
  };
  console.log(`  ${label}: ` + JSON.stringify(out));
  return out;
}

async function perf(page) {
  console.log(`\n=== PERF SWEEP (grid=${GRID}) ===`);
  const rows = [];
  for (const count of COUNTS) {
    const base = {
      "grid.res_x": GRID, "grid.res_y": GRID, "grid.res_z": GRID,
      "particles.count": count,
      "particles.density": 8,
      "render.hero.debug_view": 0,
    };
    // sort OFF baseline
    rows.push(await measure(page, { ...base, "dev.particle_sort": 0 }, `count=${count} sortOFF`));
    // sort ON across cadences
    for (const N of CADENCES) {
      rows.push(await measure(page, {
        ...base, "dev.particle_sort": 1, "dev.particle_sort_period": N,
      }, `count=${count} sortON N=${N}`));
    }
  }
  writeFileSync(join(OUT, "perf.json"), JSON.stringify(rows, null, 2));
  // Pretty table
  console.log("\n--- TABLE ---");
  console.log("config".padEnd(28), "particles".padStart(11), "scatter".padStart(8), "g2p".padStart(7), "sort".padStart(6), "sim_ms".padStart(8), "fps".padStart(6));
  for (const r of rows) {
    const n = (v, w, d = 1) => (v == null ? "-" : v.toFixed(d)).padStart(w);
    console.log(r.label.padEnd(28), String(r.particles ?? "-").padStart(11), n(r.scatter, 8), n(r.g2p, 7), n(r.sort, 6), n(r.sim_ms, 8), n(r.fps, 6, 0));
  }
  return rows;
}

// DUEL: at ONE count, alternate OFF and ON(N) for REPS rounds with short windows
// so slow thermal/clock drift hits both arms equally. Reports the paired median
// sim_ms/scatter/g2p and the per-round OFF-minus-ON delta (drift-robust verdict).
async function duel(page) {
  const count = parseInt(process.env.DUEL_COUNT || "13400000", 10);
  const N = parseInt(process.env.DUEL_N || "8", 10);
  const reps = parseInt(process.env.DUEL_REPS || "6", 10);
  console.log(`\n=== DUEL count=${count} ON(N=${N}) vs OFF, ${reps} interleaved rounds ===`);
  const base = {
    "grid.res_x": GRID, "grid.res_y": GRID, "grid.res_z": GRID,
    "particles.count": count, "particles.density": 8, "render.hero.debug_view": 0,
  };
  const offS = [], onS = [];
  for (let r = 0; r < reps; r++) {
    const off = await measure(page, { ...base, "dev.particle_sort": 0 }, `r${r} OFF`);
    const on = await measure(page, { ...base, "dev.particle_sort": 1, "dev.particle_sort_period": N }, `r${r} ON`);
    offS.push(off.sim_ms); onS.push(on.sim_ms);
    console.log(`  round ${r}: OFF sim=${off.sim_ms} ON sim=${on.sim_ms} delta(OFF-ON)=${(off.sim_ms - on.sim_ms).toFixed(1)}`);
  }
  const med = (a) => { const s = a.filter(Number.isFinite).sort((x, y) => x - y); return s.length ? s[Math.floor(s.length / 2)] : null; };
  const dOff = med(offS), dOn = med(onS);
  console.log(`  MEDIAN OFF sim=${dOff?.toFixed(1)}  ON sim=${dOn?.toFixed(1)}  ON faster by ${(dOff - dOn).toFixed(1)}ms (${(100 * (dOff - dOn) / dOff).toFixed(1)}%)`);
  writeFileSync(join(OUT, `duel_${count}_N${N}.json`), JSON.stringify({ count, N, offS, onS, dOff, dOn }, null, 2));
}

const { default: puppeteer } = await import("puppeteer-core");
const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: "new",
  args: ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-angle=default", "--no-sandbox", "--window-size=1280,800"],
});
let detPass = null;
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
  page.on("console", (m) => { const t = m.text(); if (/error|panic|validation|lost|wgsl|shader|pipeline|workgroup|storage/i.test(t)) console.log("[page] " + t); });
  page.on("pageerror", (e) => console.log("[pageerror] " + e.message));
  await page.goto(URL, { waitUntil: "networkidle2", timeout: 30000 });
  await sleep(SETTLE);
  if (!(await page.evaluate(() => !!window.__fluid))) throw new Error("__fluid never appeared");

  if (MODE === "det" || MODE === "all") detPass = await determinism(page);
  if (MODE === "perf" || MODE === "all") await perf(page);
  if (MODE === "duel") await duel(page);
} finally {
  await browser.close();
}
if (detPass === false) process.exit(2);
