import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const HERE = dirname(fileURLToPath(import.meta.url));
const URL = "http://localhost:5184/";
const CHROME = "C:/Program Files/Google/Chrome/Application/chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const { default: puppeteer } = await import("puppeteer-core");
const browser = await puppeteer.launch({
  executablePath: CHROME, headless: "new",
  args: ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-angle=default", "--no-sandbox"],
});
try {
  const page = await browser.newPage();
  page.on("console", (m) => console.log("[c:" + m.type() + "] " + m.text()));
  page.on("pageerror", (e) => console.log("[pageerror] " + e.message));
  await page.goto(URL, { waitUntil: "networkidle2", timeout: 30000 });
  await sleep(5000);
  // Turn the sort on with a small grid so the pipeline compiles; capture the
  // FIRST validation error from Dawn.
  const r = await page.evaluate(() => {
    const f = window.__fluid;
    f.set_setting("grid.res_x", 64); f.set_setting("grid.res_y", 64); f.set_setting("grid.res_z", 64);
    f.set_setting("particles.count", 0);
    f.set_setting("particles.density", 8);
    f.set_setting("dev.particle_sort", 1);
    f.set_setting("dev.particle_sort_period", 1);
    const ok = f.reset();
    return { ok, status: window.__fluidGpuStatus ? window.__fluidGpuStatus() : null };
  });
  console.log("RESET -> " + JSON.stringify(r));
  await sleep(2500);
} finally { await browser.close(); }
