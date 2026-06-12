// Browser capture harness for visible-win evidence and checkpoint bundles.
//
// Runs on the WINDOWS side (real-GPU Chrome) via puppeteer-core, pointed at the
// static dev server running inside WSL. Captures: console output (incl. the Rust
// boot diagnostics, smoke-test result, and profiler logs), page errors, and a
// PNG screenshot of the canvas after a warm-up period.
//
// Usage (from Windows node):
//   node tools/capture.mjs <url> <out.png> [waitMs] [chromePath]
// Assertion-only mode (no Chrome launch):
//   FLUID_ASSERT_TEST_STATS='{"timing":"cpu-wallclock"}' FLUID_ASSERT_REQUIRE_GPU_TIMESTAMP=1 node tools/capture.mjs
//
// Output location: a BARE filename (e.g. `boot.png`) is written into the repo's
// `captures/` dir (gitignored), anchored to THIS script's location — so it lands
// there no matter what cwd the harness was launched from. Pass a path with a
// directory (or an absolute path) to override. Console text is written alongside
// the PNG as <out>.console.txt.

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve, isAbsolute } from "node:path";
import { fileURLToPath } from "node:url";

// Repo `captures/` dir, resolved relative to this file (app/tools/ → ../../captures),
// not the cwd. Keeps screenshots out of app/tools/ (which is tracked by git).
const CAPTURES_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../../captures");

const url = process.argv[2] || "http://localhost:5184/";
const outArg = process.argv[3] || "capture.png";
// Bare filename → captures/; an explicit path (has a separator or is absolute) is
// respected as-is (relative to cwd).
const outPng =
  isAbsolute(outArg) || outArg.includes("/") || outArg.includes("\\")
    ? outArg
    : join(CAPTURES_DIR, outArg);
const waitMs = parseInt(process.argv[4] || "6000", 10);
const chromePathArg = process.argv[5] && process.argv[5] !== '""' ? process.argv[5] : "";
const chromePath =
  chromePathArg ||
  "C:/Program Files/Google/Chrome/Application/chrome.exe";
let evalSnippet = process.env.EVAL || process.argv[6] || "";
if (
  evalSnippet.length >= 2 &&
  ((evalSnippet.startsWith('"') && evalSnippet.endsWith('"')) ||
    (evalSnippet.startsWith("'") && evalSnippet.endsWith("'")))
) {
  evalSnippet = evalSnippet.slice(1, -1);
}
const viewportWidth = parseInt(process.env.VIEWPORT_WIDTH || process.argv[7] || "1280", 10);
const viewportHeight = parseInt(process.env.VIEWPORT_HEIGHT || process.argv[8] || "800", 10);
const statsPollIntervalMs = Math.max(
  50,
  parseInt(process.env.STATS_POLL_INTERVAL_MS || "250", 10) || 250,
);
const timingRank = {
  "cpu-wallclock": 0,
  "coarse-fence": 1,
  "gpu-timestamp": 2,
};
const assertions = readAssertions();
const scaleMeasurementRequested = Boolean(process.env.PARTICLES || process.env.DETAILED === "1");

const consoleLines = [];
const pageErrors = [];
const requestFailures = [];
const webGpuValidationConsoleLines = [];
const webGpuDeviceLossConsoleLines = [];
const traceSamples = [];
let liquidCellsBaseline = null;
let latestOccupiedCellDrift = null;

function record(line) {
  consoleLines.push(line);
  console.log(line);
}

function isWebGpuValidationConsoleFailure(type, text) {
  if (!["warning", "error", "assert"].includes(type)) return false;
  return (
    /Error while parsing WGSL/i.test(text) ||
    /Invalid ShaderModule|Invalid ComputePipeline|Invalid CommandBuffer/i.test(text) ||
    /CreateShaderModule|CreateComputePipeline/i.test(text) ||
    /WebGPU: too many warnings/i.test(text) ||
    /GPUValidationError|WGSL validation|WebGPU.*validation|validation.*WebGPU/i.test(text) ||
    /WebGPU.*pipeline|pipeline.*WebGPU|createComputePipeline|createShaderModule/i.test(text) ||
    /invalid command buffer/i.test(text)
  );
}

function isWebGpuDeviceLossConsoleFailure(type, text) {
  if (!["warning", "error", "assert"].includes(type)) return false;
  return (
    /device[- ]?lost|lost device|GPUDeviceLost|DeviceLost/i.test(text) ||
    /gpu.*device.*lost|webgpu.*device.*lost/i.test(text)
  );
}

function envFlag(name) {
  return /^(1|true|yes|on)$/i.test(process.env[name] || "");
}

function envNumber(name) {
  if (!process.env[name]) return null;
  const n = Number(process.env[name]);
  if (!Number.isFinite(n)) throw new Error(`${name} must be a finite number`);
  return n;
}

function readAssertions() {
  const minTimingSource = process.env.FLUID_ASSERT_MIN_TIMING_SOURCE || "";
  if (minTimingSource && timingRank[minTimingSource] == null) {
    throw new Error(
      "FLUID_ASSERT_MIN_TIMING_SOURCE must be cpu-wallclock, coarse-fence, or gpu-timestamp",
    );
  }
  return {
    minTimingSource,
    maxFrameAvgMs: envNumber("FLUID_ASSERT_MAX_FRAME_AVG_MS"),
    maxP95Ms: envNumber("FLUID_ASSERT_MAX_P95_MS"),
    maxGpuSimMs: envNumber("FLUID_ASSERT_MAX_GPU_SIM_MS"),
    maxGpuRenderMs: envNumber("FLUID_ASSERT_MAX_GPU_RENDER_MS"),
    requireScaleStatusOk: envFlag("FLUID_ASSERT_SCALE_STATUS_OK"),
    requireGpuStats: envFlag("FLUID_ASSERT_REQUIRE_GPU_STATS"),
    requireGpuTimestamp: envFlag("FLUID_ASSERT_REQUIRE_GPU_TIMESTAMP"),
  };
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

function envDurationMs(name, defaultValue) {
  const raw = process.env[name];
  const n = parseInt(raw || `${defaultValue}`, 10);
  if (!Number.isFinite(n) || n < 0) {
    throw new Error(`${name} must be a non-negative integer duration in ms`);
  }
  return n;
}

async function readPageStats(page) {
  return await page.evaluate(() =>
    window.__fluid ? JSON.parse(window.__fluid.stats_json()) : null,
  );
}

async function readPageShellState(page) {
  return await page.evaluate(() =>
    window.__fluidShell?.state ? window.__fluidShell.state() : null,
  );
}

function updateOccupiedCellDrift(stats) {
  const liquidCells = stats?.gpu?.liquid_cells;
  if (!Number.isFinite(liquidCells)) return null;
  if (liquidCellsBaseline == null) {
    liquidCellsBaseline = liquidCells;
  }
  const delta = liquidCells - liquidCellsBaseline;
  const ratio =
    liquidCellsBaseline > 0 ? delta / liquidCellsBaseline : liquidCells === 0 ? 0 : null;
  latestOccupiedCellDrift = {
    kind: "occupied_cell_count_proxy",
    baseline_liquid_cells: liquidCellsBaseline,
    final_liquid_cells: liquidCells,
    delta_liquid_cells: delta,
    ratio,
    note: "Proxy from throttled gpu.liquid_cells samples; not physical volume.",
  };
  return latestOccupiedCellDrift;
}

function compactTraceSample(label, tMs, stats) {
  const drift = updateOccupiedCellDrift(stats);
  return {
    label,
    t_ms: tMs,
    timing: stats?.timing ?? null,
    frame_avg_ms: stats?.frame_avg_ms ?? null,
    p95_ms: stats?.p95 ?? null,
    scale_status: stats?.scale_status ?? null,
    gpu_sim_ms: stats?.gpu?.sim_ms ?? null,
    gpu_render_ms: stats?.gpu?.render_ms ?? null,
    liquid_cells: stats?.gpu?.liquid_cells ?? null,
    occupied_cell_drift_ratio: drift?.ratio ?? null,
  };
}

async function pollStatsDuring(page, waitMs, label) {
  const start = Date.now();
  while (Date.now() - start < waitMs) {
    const elapsed = Date.now() - start;
    const stats = await readPageStats(page);
    traceSamples.push(compactTraceSample(label, elapsed, stats));
    await sleep(Math.min(statsPollIntervalMs, Math.max(0, waitMs - (Date.now() - start))));
  }
}

function assertNumberAtMost(failures, stats, field, value, max, label) {
  if (max == null) return;
  if (!Number.isFinite(value)) {
    failures.push(`${label} unavailable for ${field}`);
  } else if (value > max) {
    failures.push(`${field} ${value} exceeds ${max}`);
  }
}

function requireGpuTimestampStats(failures, stats, reason) {
  if (stats?.timing !== "gpu-timestamp") {
    failures.push(
      `${reason} requires stats.timing === "gpu-timestamp" (got ${stats?.timing ?? "null"})`,
    );
    return false;
  }
  if (stats.gpu == null) {
    failures.push(`${reason} requires non-null stats.gpu`);
    return false;
  }
  return true;
}

function collectAssertionFailures(stats) {
  const failures = [];
  if (assertions.minTimingSource) {
    const actual = stats?.timing ?? "";
    if (timingRank[actual] == null || timingRank[actual] < timingRank[assertions.minTimingSource]) {
      failures.push(
        `timing source ${actual || "null"} is below ${assertions.minTimingSource}`,
      );
    }
  }
  assertNumberAtMost(
    failures,
    stats,
    "frame_avg_ms",
    stats?.frame_avg_ms,
    assertions.maxFrameAvgMs,
    "frame average",
  );
  assertNumberAtMost(failures, stats, "p95", stats?.p95, assertions.maxP95Ms, "frame p95");
  if (assertions.requireScaleStatusOk && stats?.scale_status !== "ok") {
    failures.push(`scale_status ${stats?.scale_status ?? "null"} is not ok`);
  }
  if (assertions.requireGpuStats && stats?.gpu == null) {
    failures.push("stats.gpu is null");
  }
  if (assertions.requireGpuTimestamp) {
    requireGpuTimestampStats(failures, stats, "FLUID_ASSERT_REQUIRE_GPU_TIMESTAMP");
  }
  if (
    assertions.maxGpuSimMs != null &&
    requireGpuTimestampStats(failures, stats, "FLUID_ASSERT_MAX_GPU_SIM_MS")
  ) {
    assertNumberAtMost(
      failures,
      stats,
      "gpu.sim_ms",
      stats.gpu?.sim_ms,
      assertions.maxGpuSimMs,
      "GPU sim time",
    );
  }
  if (
    assertions.maxGpuRenderMs != null &&
    requireGpuTimestampStats(failures, stats, "FLUID_ASSERT_MAX_GPU_RENDER_MS")
  ) {
    assertNumberAtMost(
      failures,
      stats,
      "gpu.render_ms",
      stats.gpu?.render_ms,
      assertions.maxGpuRenderMs,
      "GPU render time",
    );
  }
  return failures;
}

if (process.env.FLUID_ASSERT_TEST_STATS) {
  const stats = JSON.parse(process.env.FLUID_ASSERT_TEST_STATS);
  const failures = collectAssertionFailures(stats);
  if (failures.length > 0) {
    console.error("[harness] assertion self-test failed: " + failures.join(", "));
    process.exit(1);
  }
  console.log("[harness] assertion self-test passed");
  process.exit(0);
}

mkdirSync(dirname(outPng), { recursive: true });
const { default: puppeteer } = await import("puppeteer-core");

const browser = await puppeteer.launch({
  executablePath: chromePath,
  headless: "new",
  args: [
    "--enable-unsafe-webgpu",
    "--enable-features=Vulkan",
    "--use-angle=default",
    "--no-sandbox",
    `--window-size=${viewportWidth},${viewportHeight}`,
  ],
});

try {
  const page = await browser.newPage();
  await page.setViewport({ width: viewportWidth, height: viewportHeight, deviceScaleFactor: 1 });

  page.on("console", (msg) => {
    const type = msg.type();
    const text = msg.text();
    const line = "[console:" + type + "] " + text;
    record(line);
    if (isWebGpuValidationConsoleFailure(type, text)) {
      webGpuValidationConsoleLines.push(line);
    }
    if (isWebGpuDeviceLossConsoleFailure(type, text)) {
      webGpuDeviceLossConsoleLines.push(line);
    }
  });
  page.on("pageerror", (err) => {
    pageErrors.push(err.message);
    record("[pageerror] " + err.message);
  });
  page.on("requestfailed", (req) => {
    const line = req.url() + " " + (req.failure()?.errorText || "");
    requestFailures.push(line);
    record("[requestfailed] " + line);
  });

  const captureStartMs = Date.now();
  record("[harness] navigating to " + url);
  await page.goto(url, { waitUntil: "networkidle2", timeout: 30000 });
  await sleep(waitMs);

  // Repeatable scale/profiler measurement path. Keep this separate from EVAL so
  // Windows cmd.exe quoting cannot silently drop the requested configuration.
  if (scaleMeasurementRequested) {
    if (process.env.PARTICLES && !/^\d+$/.test(process.env.PARTICLES)) {
      throw new Error("PARTICLES must be an integer");
    }
    const requestedParticles = process.env.PARTICLES
      ? parseInt(process.env.PARTICLES, 10)
      : null;
    const detailed = process.env.DETAILED === "1";
    const applied = await page.evaluate(
      ({ requestedParticles, detailed }) => {
        if (!window.__fluid) throw new Error("window.__fluid unavailable");
        if (requestedParticles != null) {
          window.__fluid.set_setting("particles.count", requestedParticles);
        }
        if (detailed) {
          window.__fluid.set_setting("dev.detailed_gpu_profiling", 1);
        }
        const resetOk = window.__fluid.reset();
        if (!resetOk) throw new Error("requested reset was rejected");
        return { requestedParticles, detailed };
      },
      { requestedParticles, detailed },
    );
    record("[harness] scale config -> " + JSON.stringify(applied));
    liquidCellsBaseline = null;
    latestOccupiedCellDrift = null;
    await pollStatsDuring(page, envDurationMs("MEASURE_WAIT", 12000), "measure");
  }

  // Optional: run a JS snippet in the page (e.g. drive reset) then settle.
  if (evalSnippet) {
    const out = await page.evaluate(evalSnippet);
    record("[harness] EVAL -> " + JSON.stringify(out));
    await sleep(parseInt(process.env.EVAL_WAIT || "1500", 10));
  }

  // Optional: drag across the canvas to exercise the orbit camera (DRAG=1).
  if (process.env.DRAG === "1") {
    const box = await page.evaluate(() => {
      const c = document.getElementById("gpu-canvas");
      const r = c.getBoundingClientRect();
      return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
    });
    await page.mouse.move(box.x, box.y);
    await page.mouse.down();
    for (let i = 1; i <= 20; i++) {
      await page.mouse.move(box.x + i * 9, box.y + i * 3);
    }
    await page.mouse.up();
    record("[harness] performed orbit drag");
    await sleep(500);
  }

  if (!scaleMeasurementRequested && process.env.MEASURE_WAIT) {
    await pollStatsDuring(page, envDurationMs("MEASURE_WAIT", 0), "measure");
  }

  // Optional: capture a frame sequence (for GIF encoding) into <outPng>.frames/.
  if (process.env.FRAMES) {
    const n = parseInt(process.env.FRAMES, 10);
    const interval = parseInt(process.env.FRAME_INTERVAL || "60", 10);
    const dir = outPng + ".frames";
    mkdirSync(dir, { recursive: true });
    if (process.env.SEQ_RESET) await page.evaluate("window.__fluid.reset()");
    for (let i = 0; i < n; i++) {
      await page.screenshot({ path: `${dir}/f_${String(i).padStart(4, "0")}.png` });
      await sleep(interval);
    }
    record("[harness] captured " + n + " frames to " + dir);
  }

  await page.screenshot({ path: outPng });
  record("[harness] screenshot written: " + outPng);

  // Report WebGPU availability as seen by the page, for honesty.
  const gpu = await page.evaluate(() => ({
    hasGpu: "gpu" in navigator,
    ua: navigator.userAgent,
  }));
  record("[harness] navigator.gpu present: " + gpu.hasGpu);
  record("[harness] UA: " + gpu.ua);
  const stats = await readPageStats(page);
  const shellState = await readPageShellState(page);
  traceSamples.push(compactTraceSample("final", Date.now() - captureStartMs, stats));
  record("[harness] stats_json: " + JSON.stringify(stats));
  record("[harness] shell_state: " + JSON.stringify(shellState));
  if (latestOccupiedCellDrift) {
    record("[harness] occupied_cell_drift_proxy: " + JSON.stringify(latestOccupiedCellDrift));
  }

  writeFileSync(outPng + ".console.txt", consoleLines.join("\n") + "\n");
  writeFileSync(
    outPng + ".trace.ndjson",
    traceSamples.map((sample) => JSON.stringify(sample)).join("\n") + "\n",
  );
  writeFileSync(
    outPng + ".stats.json",
    JSON.stringify(
      {
        url,
        out_png: outPng,
        viewport: { width: viewportWidth, height: viewportHeight },
        stats_poll_interval_ms: statsPollIntervalMs,
        sample_count: traceSamples.length,
        final_stats: stats,
        final_shell_state: shellState,
        occupied_cell_drift_proxy: latestOccupiedCellDrift,
      },
      null,
      2,
    ) + "\n",
  );
  const smokeFailed = consoleLines.some((line) => line.includes("[fluid-lab][smoke] FAIL"));
  const failures = [];
  if (!gpu.hasGpu) failures.push("navigator.gpu is false");
  if (stats == null) failures.push("stats_json unavailable");
  if (pageErrors.length > 0) failures.push(`${pageErrors.length} page error(s)`);
  if (requestFailures.length > 0) failures.push(`${requestFailures.length} failed request(s)`);
  if (webGpuValidationConsoleLines.length > 0) {
    failures.push(`${webGpuValidationConsoleLines.length} WebGPU validation/pipeline warning(s)`);
  }
  if (webGpuDeviceLossConsoleLines.length > 0) {
    failures.push(`${webGpuDeviceLossConsoleLines.length} WebGPU device-loss warning(s)`);
  }
  if (["device-lost", "validation-error"].includes(shellState?.gpuDeviceStatus)) {
    failures.push(`gpuDeviceStatus is ${shellState.gpuDeviceStatus}`);
  }
  if (smokeFailed) failures.push("WebGPU smoke test failed");
  failures.push(...collectAssertionFailures(stats));
  if (failures.length > 0) {
    throw new Error("capture failed acceptance checks: " + failures.join(", "));
  }
} finally {
  await browser.close();
}
