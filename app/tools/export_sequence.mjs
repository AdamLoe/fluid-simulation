// Headless PNG-sequence exporter for explicit fixed-step fluid captures.
//
// Runs on the WINDOWS side (real-GPU Chrome) via puppeteer-core, pointed at the
// static dev server running inside WSL. The browser shell enters export mode so
// its normal rAF loop stops advancing the simulation; this tool then calls the
// explicit WASM export frame bridge once per PNG.
//
// Usage:
//   node tools/export_sequence.mjs [url] [outDir] [frameCount] [outputFps] [simSecondsPerFrame] [viewportWidth] [viewportHeight] [chromePath] [configPath]
//
// Bare outDir names are written under the repo captures/ dir. configPath is an
// optional JSON file containing either {settings:{id:value}}, an array of
// [id,value] entries, or a plain {id:value} map.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const CAPTURES_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../../captures");

const url = process.argv[2] || "http://localhost:5184/";
const outArg = process.argv[3] || "export-sequence";
const outDir =
  isAbsolute(outArg) || outArg.includes("/") || outArg.includes("\\")
    ? outArg
    : join(CAPTURES_DIR, outArg);
const frameCount = integerArg(process.argv[4] || "3", "frameCount", { min: 1 });
const outputFps = numberArg(process.argv[5] || "60", "outputFps", { minExclusive: 0 });
const simSecondsPerFrame = numberArg(
  process.argv[6] || String(1 / outputFps),
  "simSecondsPerFrame",
  { minExclusive: 0 },
);
const viewportWidth = integerArg(
  process.env.VIEWPORT_WIDTH || process.argv[7] || "1280",
  "viewportWidth",
  { min: 1 },
);
const viewportHeight = integerArg(
  process.env.VIEWPORT_HEIGHT || process.argv[8] || "800",
  "viewportHeight",
  { min: 1 },
);
const chromePathArg = process.argv[9] && process.argv[9] !== '""' ? process.argv[9] : "";
const chromePath = chromePathArg || "C:/Program Files/Google/Chrome/Application/chrome.exe";
const configPath = process.argv[10] || process.env.FLUID_EXPORT_CONFIG_PATH || "";
const configEntries = readConfigEntries(configPath);

const consoleLines = [];
const pageErrors = [];
const requestFailures = [];
const webGpuValidationConsoleLines = [];
const webGpuDeviceLossConsoleLines = [];
const traceSamples = [];

function record(line) {
  consoleLines.push(line);
  console.log(line);
}

function integerArg(raw, name, { min = null } = {}) {
  const n = Number.parseInt(raw, 10);
  if (!Number.isInteger(n) || String(raw).trim() === "") {
    throw new Error(`${name} must be an integer`);
  }
  if (min != null && n < min) {
    throw new Error(`${name} must be >= ${min}`);
  }
  return n;
}

function numberArg(raw, name, { minExclusive = null } = {}) {
  const n = Number(raw);
  if (!Number.isFinite(n)) {
    throw new Error(`${name} must be a finite number`);
  }
  if (minExclusive != null && n <= minExclusive) {
    throw new Error(`${name} must be > ${minExclusive}`);
  }
  return n;
}

function readConfigEntries(path) {
  const raw = process.env.FLUID_EXPORT_CONFIG || (path ? readFileSync(path, "utf8") : "");
  if (!raw.trim()) return [];
  const parsed = JSON.parse(raw);
  if (Array.isArray(parsed)) return parsed.map(normalizeEntry);
  const settings = parsed?.settings && typeof parsed.settings === "object" ? parsed.settings : parsed;
  if (!settings || typeof settings !== "object") {
    throw new Error("export config must be an object or array");
  }
  return Object.entries(settings).map(normalizeEntry);
}

function normalizeEntry(entry) {
  if (!Array.isArray(entry) || entry.length !== 2) {
    throw new Error("export config array entries must be [id, value]");
  }
  const [id, value] = entry;
  const numericValue = Number(value);
  if (typeof id !== "string" || id.length === 0 || !Number.isFinite(numericValue)) {
    throw new Error("export config entries must be [non-empty string, finite number]");
  }
  return [id, numericValue];
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

function framePath(index) {
  return join(outDir, `frame_${String(index).padStart(5, "0")}.png`);
}

function compactTraceSample(index, exportResult, stats, shellState) {
  return {
    frame_index: index,
    export_result: exportResult,
    timing: stats?.timing ?? null,
    timestep_policy: stats?.timestep_policy ?? null,
    substeps_this_frame: stats?.substeps_this_frame ?? null,
    sim_advanced_ms: stats?.sim_advanced_ms ?? null,
    gpu_device_status: shellState?.gpuDeviceStatus ?? stats?.gpu_device_status ?? null,
  };
}

function assertIntegerSubsteps(fixedDt) {
  const ratio = simSecondsPerFrame / fixedDt;
  const substeps = Math.round(ratio);
  if (!Number.isFinite(ratio) || substeps < 0 || Math.abs(ratio - substeps) > 1.0e-3) {
    throw new Error(
      `simSecondsPerFrame (${simSecondsPerFrame}) must be an integer multiple of active physics.fixed_dt (${fixedDt}); ratio=${ratio}`,
    );
  }
  return substeps;
}

mkdirSync(outDir, { recursive: true });
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

let metadata = null;

try {
  const page = await browser.newPage();
  await page.setViewport({ width: viewportWidth, height: viewportHeight, deviceScaleFactor: 1 });

  page.on("console", (msg) => {
    const type = msg.type();
    const text = msg.text();
    const line = "[console:" + type + "] " + text;
    record(line);
    if (isWebGpuValidationConsoleFailure(type, text)) webGpuValidationConsoleLines.push(line);
    if (isWebGpuDeviceLossConsoleFailure(type, text)) webGpuDeviceLossConsoleLines.push(line);
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

  record("[export] navigating to " + url);
  await page.goto(url, { waitUntil: "networkidle2", timeout: 30000 });
  await page.waitForFunction(
    () => Boolean(window.__fluid && window.__fluidShell?.state && window.__fluidShell?.exportFrame),
    { timeout: 30000 },
  );

  const setup = await page.evaluate((entries) => {
    const shell = window.__fluidShell;
    shell.beginExportMode();
    const applyResult = entries.length ? shell.applySettings(entries, "export sequence") : null;
    const resetOk = shell.resetForExport();
    if (!resetOk) throw new Error("requested reset was rejected");
    shell.beginExportMode();
    const fixedDt = shell.setting("physics.fixed_dt")?.value;
    return {
      applyResult,
      fixedDt,
      config: shell.exportConfig(),
      shellState: shell.state(),
      stats: JSON.parse(window.__fluid.stats_json()),
    };
  }, configEntries);

  const fixedDt = Number(setup.fixedDt);
  if (!Number.isFinite(fixedDt) || fixedDt <= 0) {
    throw new Error("active physics.fixed_dt unavailable after setup");
  }
  const substepsPerFrame = assertIntegerSubsteps(fixedDt);
  record(
    `[export] setup fixed_dt=${fixedDt} sim_seconds_per_frame=${simSecondsPerFrame} substeps=${substepsPerFrame}`,
  );

  for (let i = 0; i < frameCount; i++) {
    const exportResult = await page.evaluate(
      ({ substeps, simSeconds }) => window.__fluidShell.exportFrame(substeps, simSeconds),
      { substeps: substepsPerFrame, simSeconds: simSecondsPerFrame },
    );
    if (!exportResult?.ok) {
      throw new Error(`export_frame failed at frame ${i}: ${JSON.stringify(exportResult)}`);
    }
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => resolve())));
    const stats = await readPageStats(page);
    const shellState = await readPageShellState(page);
    traceSamples.push(compactTraceSample(i, exportResult, stats, shellState));
    await page.screenshot({ path: framePath(i) });
    record(`[export] frame ${i + 1}/${frameCount} -> ${framePath(i)}`);
  }

  const gpu = await page.evaluate(() => ({
    hasGpu: "gpu" in navigator,
    ua: navigator.userAgent,
  }));
  const finalStats = await readPageStats(page);
  const finalShellState = await readPageShellState(page);
  record("[export] navigator.gpu present: " + gpu.hasGpu);
  record("[export] UA: " + gpu.ua);
  record("[export] stats_json: " + JSON.stringify(finalStats));
  record("[export] shell_state: " + JSON.stringify(finalShellState));

  metadata = {
    url,
    out_dir: outDir,
    frame_count: frameCount,
    output_fps: outputFps,
    sim_seconds_per_frame: simSecondsPerFrame,
    substeps_per_frame: substepsPerFrame,
    fixed_dt: fixedDt,
    viewport: { width: viewportWidth, height: viewportHeight, device_scale_factor: 1 },
    settings_config: setup.config,
    applied_config_entries: configEntries,
    apply_result: setup.applyResult,
    final_stats_json: finalStats,
    final_shell_state: finalShellState,
    timing_source: finalStats?.timing ?? null,
    gpu_status: finalShellState?.gpuDeviceStatus ?? finalStats?.gpu_device_status ?? null,
    gpu,
    tool_decisions: {
      output: "png-sequence",
      video_encoding: "out-of-scope",
      browser_ui: "not-added",
      supersampling: "out-of-scope",
      camera_paths: "out-of-scope",
      audio: "out-of-scope",
      cloud_rendering: "out-of-scope",
      stepping: "explicit-fixed-substeps",
      normal_raf_loop: "bypassed-by-shell-export-mode",
      screenshot_source: "real-gpu-headless-chrome-viewport",
    },
    frames: Array.from({ length: frameCount }, (_, i) => framePath(i)),
  };

  const smokeFailed = consoleLines.some((line) => line.includes("[fluid-lab][smoke] FAIL"));
  const failures = [];
  if (!gpu.hasGpu) failures.push("navigator.gpu is false");
  if (finalStats == null) failures.push("stats_json unavailable");
  if (pageErrors.length > 0) failures.push(`${pageErrors.length} page error(s)`);
  if (requestFailures.length > 0) failures.push(`${requestFailures.length} failed request(s)`);
  if (webGpuValidationConsoleLines.length > 0) {
    failures.push(`${webGpuValidationConsoleLines.length} WebGPU validation/pipeline warning(s)`);
  }
  if (webGpuDeviceLossConsoleLines.length > 0) {
    failures.push(`${webGpuDeviceLossConsoleLines.length} WebGPU device-loss warning(s)`);
  }
  if (["device-lost", "surface-validation-error"].includes(metadata.gpu_status)) {
    failures.push(`gpuDeviceStatus is ${metadata.gpu_status}`);
  }
  if (setup.applyResult?.resetRejected) failures.push("setting reset was rejected");
  if (smokeFailed) failures.push("WebGPU smoke test failed");

  writeFileSync(join(outDir, "console.txt"), consoleLines.join("\n") + "\n");
  writeFileSync(
    join(outDir, "trace.ndjson"),
    traceSamples.map((sample) => JSON.stringify(sample)).join("\n") + "\n",
  );
  writeFileSync(join(outDir, "metadata.json"), JSON.stringify(metadata, null, 2) + "\n");

  if (failures.length > 0) {
    throw new Error("export failed acceptance checks: " + failures.join(", "));
  }
} finally {
  if (metadata == null) {
    writeFileSync(join(outDir, "console.txt"), consoleLines.join("\n") + "\n");
    writeFileSync(
      join(outDir, "trace.ndjson"),
      traceSamples.map((sample) => JSON.stringify(sample)).join("\n") + "\n",
    );
  }
  await browser.close();
}
