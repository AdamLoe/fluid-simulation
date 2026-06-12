// Browser capture harness for visible-win evidence and checkpoint bundles.
//
// Runs on the WINDOWS side (real-GPU Chrome) via puppeteer-core, pointed at the
// static dev server running inside WSL. Captures: console output (incl. the Rust
// boot diagnostics, smoke-test result, and profiler logs), page errors, and a
// PNG screenshot of the canvas after a warm-up period.
//
// Usage (from Windows node):
//   node tools/capture.mjs <url> <out.png> [waitMs] [chromePath]
//
// Output location: a BARE filename (e.g. `boot.png`) is written into the repo's
// `captures/` dir (gitignored), anchored to THIS script's location — so it lands
// there no matter what cwd the harness was launched from. Pass a path with a
// directory (or an absolute path) to override. Console text is written alongside
// the PNG as <out>.console.txt.

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve, isAbsolute } from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";

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
mkdirSync(dirname(outPng), { recursive: true });
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

const consoleLines = [];
const pageErrors = [];
const requestFailures = [];

function record(line) {
  consoleLines.push(line);
  console.log(line);
}

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

  page.on("console", (msg) => record("[console:" + msg.type() + "] " + msg.text()));
  page.on("pageerror", (err) => {
    pageErrors.push(err.message);
    record("[pageerror] " + err.message);
  });
  page.on("requestfailed", (req) => {
    const line = req.url() + " " + (req.failure()?.errorText || "");
    requestFailures.push(line);
    record("[requestfailed] " + line);
  });

  record("[harness] navigating to " + url);
  await page.goto(url, { waitUntil: "networkidle2", timeout: 30000 });
  await new Promise((r) => setTimeout(r, waitMs));

  // Repeatable scale/profiler measurement path. Keep this separate from EVAL so
  // Windows cmd.exe quoting cannot silently drop the requested configuration.
  if (process.env.PARTICLES || process.env.DETAILED === "1") {
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
    await new Promise((r) =>
      setTimeout(r, parseInt(process.env.MEASURE_WAIT || "12000", 10)),
    );
  }

  // Optional: run a JS snippet in the page (e.g. drive reset) then settle.
  if (evalSnippet) {
    const out = await page.evaluate(evalSnippet);
    record("[harness] EVAL -> " + JSON.stringify(out));
    await new Promise((r) => setTimeout(r, parseInt(process.env.EVAL_WAIT || "1500", 10)));
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
    await new Promise((r) => setTimeout(r, 500));
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
      await new Promise((r) => setTimeout(r, interval));
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
  const stats = await page.evaluate(() =>
    window.__fluid ? JSON.parse(window.__fluid.stats_json()) : null,
  );
  record("[harness] stats_json: " + JSON.stringify(stats));

  writeFileSync(outPng + ".console.txt", consoleLines.join("\n") + "\n");
  const smokeFailed = consoleLines.some((line) => line.includes("[fluid-lab][smoke] FAIL"));
  const failures = [];
  if (!gpu.hasGpu) failures.push("navigator.gpu is false");
  if (stats == null) failures.push("stats_json unavailable");
  if (pageErrors.length > 0) failures.push(`${pageErrors.length} page error(s)`);
  if (requestFailures.length > 0) failures.push(`${requestFailures.length} failed request(s)`);
  if (smokeFailed) failures.push("WebGPU smoke test failed");
  if (failures.length > 0) {
    throw new Error("capture failed acceptance checks: " + failures.join(", "));
  }
} finally {
  await browser.close();
}
