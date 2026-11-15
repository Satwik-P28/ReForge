#!/usr/bin/env node
/**
 * ReForge browser investigation driver (protocol `reforge-web-driver/1`).
 *
 * One JSON object per line on stdin, one per line on stdout. Diagnostics go to
 * stderr so they can never corrupt the protocol. The driver owns the browser,
 * the static server, and reset semantics; the Rust core owns inference.
 */

import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import readline from "node:readline";
import { chromium, firefox } from "playwright";

const PROTOCOL = "reforge-web-driver/1";
const CHROMIUM =
  process.env.REFORGE_CHROMIUM ||
  path.join(
    process.env.HOME || "",
    ".cache/ms-playwright/chromium_headless_shell-1148/chrome-linux/headless_shell",
  );
const BROWSER_ENGINE = process.env.REFORGE_BROWSER === "firefox" ? "firefox" : "chromium";

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".png": "image/png",
  ".svg": "image/svg+xml",
};

let browser = null;
let context = null;
let page = null;
let server = null;
let baseUrl = null;
let appDir = null;
let entry = "index.html";
let viewport = { width: 960, height: 720 };
let consoleErrors = [];
let requests = [];

function digest(buffer) {
  return crypto.createHash("sha256").update(buffer).digest("hex");
}

function serve(dir) {
  return new Promise((resolve, reject) => {
    const srv = http.createServer((req, res) => {
      const raw = decodeURIComponent((req.url || "/").split("?")[0]);
      const relative = raw === "/" ? "/index.html" : raw;
      const resolved = path.resolve(dir, "." + relative);
      const root = path.resolve(dir);
      if (resolved !== root && !resolved.startsWith(root + path.sep)) {
        res.writeHead(403);
        res.end("forbidden");
        return;
      }
      fs.readFile(resolved, (error, data) => {
        if (error) {
          res.writeHead(404);
          res.end("not found");
          return;
        }
        res.writeHead(200, {
          "content-type": MIME[path.extname(resolved)] || "application/octet-stream",
        });
        res.end(data);
      });
    });
    srv.on("error", reject);
    srv.listen(0, "127.0.0.1", () => resolve(srv));
  });
}

async function newSession() {
  if (context) {
    await context.close();
  }
  context = await browser.newContext({ viewport });
  page = await context.newPage();
  consoleErrors = [];
  requests = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text().slice(0, 200));
  });
  page.on("requestfailed", (request) => {
    requests.push({ url: request.url().slice(0, 200), failed: true });
  });
  page.on("response", (response) => {
    requests.push({ url: response.url().slice(0, 200), status: response.status() });
  });
  await page.goto(`${baseUrl}/${entry}`, { waitUntil: "networkidle" });
  await page.waitForTimeout(60);
}

async function snapshot() {
  const aria = await page.locator("body").ariaSnapshot();
  const text = await page.locator("body").innerText();
  const storage = await page.evaluate(() => {
    const out = {};
    for (let i = 0; i < localStorage.length; i += 1) {
      out[localStorage.key(i)] = localStorage.getItem(localStorage.key(i));
    }
    return out;
  });
  const forms = await page.evaluate(() =>
    Array.from(document.querySelectorAll("input,textarea,select")).map((element) => ({
      name: element.getAttribute("aria-label") || element.name || element.id || "",
      value: String(element.value ?? ""),
    })),
  );
  const links = await page.evaluate(() =>
    Array.from(document.querySelectorAll("a")).map((a) => ({
      text: (a.textContent || "").trim().slice(0, 60),
      href: a.getAttribute("href") || "",
    })),
  );
  const shot = await page.screenshot({ fullPage: false });
  const stateDigest = digest(
    Buffer.from(aria + "\u0000" + JSON.stringify(storage) + "\u0000" + page.url()),
  );
  return {
    url: page.url(),
    title: await page.title(),
    aria,
    text: text.slice(0, 4000),
    storage,
    forms,
    links,
    console_errors: consoleErrors.slice(-8),
    requests: requests.slice(-12),
    screenshot: { digest: digest(shot), bytes: shot.length, media_type: "image/png" },
    state_digest: stateDigest,
  };
}

async function perform(input) {
  const role = input.role || "button";
  const name = input.name || "";
  const value = input.value === undefined || input.value === null ? "" : String(input.value);
  switch (input.kind) {
    case "click":
      await page.getByRole(role, { name }).first().click({ timeout: 4000 });
      break;
    case "fill":
      await page.getByRole(role, { name }).first().fill(value, { timeout: 4000 });
      break;
    case "fill_and_click":
      await page.getByRole(role, { name }).first().fill(value, { timeout: 4000 });
      await page.getByRole(input.click_role || "button", { name: input.click_name || "Add" }).first().click({ timeout: 4000 });
      break;
    case "navigate": {
      const target = new URL(value, `${baseUrl}/`).toString();
      if (!target.startsWith(baseUrl)) {
        throw new Error("navigation outside the fixture is refused");
      }
      await page.goto(target, { waitUntil: "networkidle" });
      break;
    }
    case "press":
      await page.keyboard.press(value || "Enter");
      break;
    case "submit": {
      const form = page.getByRole("form", { name }).first();
      await form.evaluate((element) => element.requestSubmit());
      break;
    }
    default:
      throw new Error(`unsupported action kind '${input.kind}'`);
  }
  await page.waitForTimeout(120);
  return snapshot();
}

async function handle(request) {
  switch (request.op) {
    case "init": {
      if (browser) {
        return { protocol: PROTOCOL, reset: true };
      }
      appDir = path.resolve(String(request.app_dir));
      entry = String(request.entry || "index.html");
      viewport = request.viewport || viewport;
      server = await serve(appDir);
      const address = server.address();
      baseUrl = `http://127.0.0.1:${address.port}`;
      browser =
        BROWSER_ENGINE === "firefox"
          ? await firefox.launch({ headless: true })
          : await chromium.launch({
              executablePath: CHROMIUM,
              args: ["--no-sandbox", "--disable-dev-shm-usage"],
            });
      await newSession();
      return {
        protocol: PROTOCOL,
        browser: BROWSER_ENGINE,
        url: baseUrl,
        snapshot: await snapshot(),
      };
    }
    case "reset": {
      await newSession();
      return await snapshot();
    }
    case "snapshot":
      return await snapshot();
    case "perform":
      return await perform(request);
    case "close": {
      if (browser) await browser.close();
      if (server) server.close();
      browser = null;
      server = null;
      setTimeout(() => process.exit(0), 50);
      return { closed: true };
    }
    default:
      throw new Error(`unknown op '${request.op}'`);
  }
}

const rl = readline.createInterface({ input: process.stdin });
rl.on("line", async (line) => {
  const text = line.trim();
  if (!text) return;
  let request;
  try {
    request = JSON.parse(text);
  } catch (error) {
    process.stdout.write(JSON.stringify({ id: null, ok: false, error: "invalid json" }) + "\n");
    return;
  }
  try {
    const data = await handle(request);
    process.stdout.write(JSON.stringify({ id: request.id, ok: true, data }) + "\n");
  } catch (error) {
    process.stdout.write(
      JSON.stringify({ id: request.id, ok: false, error: String(error && error.message ? error.message : error) }) +
        "\n",
    );
  }
});
rl.on("close", async () => {
  if (browser) await browser.close().catch(() => {});
  if (server) server.close();
  process.exit(0);
});
