// Browser capture harness for visible-win evidence and checkpoint bundles.
//
// Runs on the WINDOWS side (real-GPU Chrome) via puppeteer-core, pointed at the
// Vite dev server running inside WSL. Captures: console output (incl. the Rust
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

const url = process.argv[2] || "http://localhost:5173/";
const outArg = process.argv[3] || "capture.png";
// Bare filename → captures/; an explicit path (has a separator or is absolute) is
// respected as-is (relative to cwd).
const outPng =
  isAbsolute(outArg) || outArg.includes("/") || outArg.includes("\\")
    ? outArg
    : join(CAPTURES_DIR, outArg);
mkdirSync(dirname(outPng), { recursive: true });
const waitMs = parseInt(process.argv[4] || "6000", 10);
const chromePath =
  process.argv[5] ||
  "C:/Program Files/Google/Chrome/Application/chrome.exe";

const consoleLines = [];

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
    "--window-size=1280,800",
  ],
});

try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });

  page.on("console", (msg) => record("[console:" + msg.type() + "] " + msg.text()));
  page.on("pageerror", (err) => record("[pageerror] " + err.message));
  page.on("requestfailed", (req) =>
    record("[requestfailed] " + req.url() + " " + (req.failure()?.errorText || "")),
  );

  record("[harness] navigating to " + url);
  await page.goto(url, { waitUntil: "networkidle2", timeout: 30000 });
  await new Promise((r) => setTimeout(r, waitMs));

  // Optional: run a JS snippet in the page (e.g. drive reset) then settle.
  if (process.env.EVAL) {
    const out = await page.evaluate(process.env.EVAL);
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

  writeFileSync(outPng + ".console.txt", consoleLines.join("\n") + "\n");
} finally {
  await browser.close();
}
