/// <reference types="node" />

import {
  test,
  expect,
  waitForAppReady,
  clearAllBrowserStorage,
} from "./fixtures";
import type { Page } from "@playwright/test";
import { spawn, type ChildProcessWithoutNullStreams } from "child_process";
import path from "path";
import { fileURLToPath } from "url";
import { existsSync, mkdtempSync, rmSync } from "fs";
import { tmpdir } from "os";

const defaultServerHost = process.env.SYNC_SERVER_HOST ?? "127.0.0.1";
const baseServerPort = Number(process.env.SYNC_SERVER_PORT ?? "3030");
// Use offset +5 to avoid collisions with other sync test suites
let serverPort = baseServerPort + 5;
let serverUrl = process.env.SYNC_SERVER_URL ?? `http://${defaultServerHost}:${serverPort}`;
const shouldStartServer = process.env.SYNC_E2E_START_SERVER !== "0";
const repoRoot = path.resolve(
  fileURLToPath(new URL("../../..", import.meta.url)),
);

const syncServerBinary =
  process.env.SYNC_SERVER_BINARY ??
  path.join(repoRoot, "target/release/diaryx_sync_server");

let serverProcess: ChildProcessWithoutNullStreams | null = null;
let serverAvailable = false;
let tempDataDir: string | null = null;

function log(label: string, msg: string): void {
  console.log(`[${label}] ${msg}`);
}

function getProjectPort(projectName: string): number {
  switch (projectName) {
    case "webkit":
      return baseServerPort + 6;
    case "firefox":
      return baseServerPort + 7;
    case "chromium":
    default:
      return baseServerPort + 5;
  }
}

async function waitForServerReady(): Promise<boolean> {
  log("server", "Waiting for server to be ready...");
  for (let attempt = 0; attempt < 40; attempt++) {
    try {
      const response = await fetch(`${serverUrl}/api/status`);
      if (response.ok) {
        log("server", `Server ready after ${attempt + 1} attempts`);
        return true;
      }
    } catch {
      // ignore and retry
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  log("server", "Server failed to become ready after 40 attempts");
  return false;
}

function setupConsoleLogs(page: Page, label: string): void {
  page.on("console", (msg) => {
    const text = msg.text();
    if (
      text.includes("[Sync") ||
      text.includes("CRDT") ||
      text.includes("WebSocket") ||
      text.includes("sync") ||
      text.includes("Auth") ||
      text.includes("[App]") ||
      text.includes("[Storage]") ||
      text.includes("workspace") ||
      text.includes("error") ||
      text.includes("Error") ||
      text.includes("UnifiedSync")
    ) {
      log(label, text);
    }
  });
}

async function enableShowAllFiles(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const { workspaceStore } = await import("/src/models/stores");
    const { refreshTree } = await import("/src/controllers/workspaceController");
    const { getBackend, createApi } = await import("/src/lib/backend");

    workspaceStore.setShowUnlinkedFiles(true);
    const backend = await getBackend();
    const api = createApi(backend);
    await refreshTree(
      api,
      backend,
      workspaceStore.showUnlinkedFiles,
      workspaceStore.showHiddenFiles,
    );
  });
}

async function waitForWorkspaceCrdtInitialized(page: Page, timeout = 30000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    const initialized = await page.evaluate(async () => {
      const { workspaceStore } = await import("/src/models/stores");
      return workspaceStore.workspaceCrdtInitialized;
    });

    if (initialized) return;
    await page.waitForTimeout(500);
  }
  throw new Error("Timed out waiting for workspace CRDT to initialize");
}

async function waitForFileExists(page: Page, entryPath: string, timeout = 30000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    const exists = await page.evaluate(async (p) => {
      const candidates = [p, p.startsWith("./") ? p.slice(2) : `./${p}`];

      // Primary: check CRDT metadata via globalThis (bypasses Vite module duplication).
      // Note: metadata may contain BigInt fields which can't cross the Playwright
      // serialization boundary, so we must resolve to a plain boolean inside evaluate.
      const bridge = (globalThis as any).__diaryx_bridge;
      if (bridge?.getFileMetadata) {
        for (const candidate of candidates) {
          try {
            const metadata = await bridge.getFileMetadata(candidate);
            if (metadata && !metadata.deleted) return true;
          } catch { /* ignore */ }
        }
      }

      // Fallback: check virtual filesystem via backend API
      try {
        const { getBackend, createApi } = await import("/src/lib/backend");
        const backend = await getBackend();
        const api = createApi(backend);
        for (const candidate of candidates) {
          try {
            if (await api.fileExists(candidate)) return true;
          } catch { /* ignore */ }
        }
      } catch { /* ignore */ }

      return false;
    }, entryPath);

    if (exists) return;
    await page.waitForTimeout(500);
  }
  throw new Error(`Timed out waiting for file to exist: ${entryPath}`);
}

async function waitForEntryContent(
  page: Page,
  entryPath: string,
  expected: string,
  timeout = 30000,
): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    const content = await page.evaluate(async (args) => {
      const candidates = [args.p, args.p.startsWith("./") ? args.p.slice(2) : `./${args.p}`];

      // Primary: use globalThis bridge directly (bypasses slow Vite module imports)
      const bridge = (globalThis as any).__diaryx_bridge;
      if (bridge?.ensureBodySync && bridge?.getBodyContentFromCrdt) {
        for (const candidate of candidates) {
          try {
            await bridge.ensureBodySync(candidate);
            const crdtContent = await bridge.getBodyContentFromCrdt(candidate);
            if (crdtContent) return crdtContent;
          } catch { /* ignore */ }
        }
      }

      // Fallback: use backend API via module imports
      try {
        const { getBackend, createApi } = await import("/src/lib/backend");
        const backend = await getBackend();
        const api = createApi(backend);
        for (const candidate of candidates) {
          try {
            const entry = await api.getEntry(candidate);
            if (entry?.content) return entry.content;
          } catch { /* ignore */ }
          try {
            const content = await api.readFile(candidate);
            if (content) return content;
          } catch { /* ignore */ }
        }
      } catch { /* ignore */ }

      return "";
    }, { p: entryPath });

    if (content.includes(expected)) return;
    await page.waitForTimeout(500);
  }
  throw new Error(`Timed out waiting for entry content "${expected}" in ${entryPath}`);
}

async function waitForFileMissing(page: Page, entryPath: string, timeout = 30000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    const exists = await page.evaluate(async (p) => {
      const candidates = [p, p.startsWith("./") ? p.slice(2) : `./${p}`];

      // Primary: check CRDT metadata via globalThis (bypasses Vite module duplication)
      const bridge = (globalThis as any).__diaryx_bridge;
      if (bridge?.getFileMetadata) {
        for (const candidate of candidates) {
          try {
            const metadata = await bridge.getFileMetadata(candidate);
            if (metadata && !metadata.deleted) return true;
          } catch { /* ignore */ }
        }
      }

      // Fallback: check virtual filesystem via backend API
      try {
        const { getBackend, createApi } = await import("/src/lib/backend");
        const backend = await getBackend();
        const api = createApi(backend);
        for (const candidate of candidates) {
          try {
            if (await api.fileExists(candidate)) return true;
          } catch { /* ignore */ }
        }
      } catch { /* ignore */ }

      return false;
    }, entryPath);

    if (!exists) return;
    await page.waitForTimeout(500);
  }
  throw new Error(`Timed out waiting for file to be missing: ${entryPath}`);
}

async function createEntry(
  page: Page,
  entryPath: string,
  title: string,
  body: string,
  parentPath: string | null = null,
): Promise<string> {
  const resolvedPath = await page.evaluate(
    async ({ path, entryTitle, entryBody, parent }) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const { refreshTree } = await import("/src/controllers/workspaceController");
      const { workspaceStore } = await import("/src/models/stores");

      const backend = await getBackend();
      const api = createApi(backend);
      let resolvedParent = parent;
      if (!resolvedParent) {
        const workspaceDir = backend
          .getWorkspacePath()
          .replace(/\/index\.md$/, "")
          .replace(/\/README\.md$/, "");
        try {
          resolvedParent = await api.findRootIndex(workspaceDir);
        } catch {
          resolvedParent = `${workspaceDir}/README.md`;
        }
      }
      await api.createEntry(path, { title: entryTitle, part_of: resolvedParent });
      await api.saveEntry(path, entryBody);

      if (resolvedParent) {
        try {
          const parentFm = await api.getFrontmatter(resolvedParent);
          const contents = (parentFm.contents as string[]) || [];
          if (!contents.includes(path)) {
            contents.push(path);
            await api.setFrontmatterProperty(resolvedParent, "contents", contents);
          }
        } catch (e) {
          console.warn(`[test] Failed to add ${path} to ${resolvedParent} contents:`, e);
        }
      }

      let resolved = path;
      const candidates = [path];
      if (path.startsWith("./")) {
        candidates.push(path.slice(2));
      } else {
        candidates.push(`./${path}`);
      }
      for (const candidate of candidates) {
        try {
          if (await api.fileExists(candidate)) {
            resolved = candidate;
            break;
          }
        } catch { /* ignore */ }
      }

      await refreshTree(
        api,
        backend,
        workspaceStore.showUnlinkedFiles,
        workspaceStore.showHiddenFiles,
      );
      return resolved;
    },
    { path: entryPath, entryTitle: title, entryBody: body, parent: parentPath },
  );

  await expect(
    page.getByRole("treeitem", { name: new RegExp(title) }),
  ).toBeVisible({ timeout: 30000 });

  return resolvedPath;
}

async function openSyncWizard(page: Page, label: string): Promise<void> {
  log(label, "Opening sync wizard");
  await page.getByLabel("Sync status").click();
  await page.getByRole("button", { name: /Set up sync|Manage sync/i }).click();
  await expect(page.getByText("Sign In to Sync")).toBeVisible({
    timeout: 20000,
  });
  log(label, "Sync wizard opened");
}

async function completeAuthAndInit(
  page: Page,
  email: string,
  modeLabel: RegExp,
  label: string,
): Promise<void> {
  await openSyncWizard(page, label);

  log(label, "Filling email and server URL");
  await page.getByLabel("Email Address").fill(email);
  const advancedButton = page.getByRole("button", { name: "Advanced" });
  await advancedButton.click({ force: true });
  await page.getByLabel("Server URL").fill(serverUrl);

  log(label, "Requesting magic link");
  const magicLinkResponsePromise = page.waitForResponse((response) => {
    return (
      response.url().includes("/auth/magic-link") &&
      response.request().method() === "POST"
    );
  });

  await page.getByRole("button", { name: /Send Sign-in Link/i }).click();

  const magicLinkResponse = await magicLinkResponsePromise;
  const magicLinkPayload = await magicLinkResponse.json();
  const devLink = magicLinkPayload?.dev_link;

  if (!devLink) {
    throw new Error(
      "Magic link dev link missing. Ensure the sync server email is not configured for tests.",
    );
  }

  const token = new URL(devLink).searchParams.get("token");
  if (!token) {
    throw new Error("Magic link token missing from dev link response.");
  }
  log(label, "Got magic link token");

  log(label, "Clicking dev link to verify token");
  const devLinkElement = page.locator('a:has-text("Click here to verify")');
  await devLinkElement.waitFor({ state: "visible", timeout: 20000 });
  await devLinkElement.click();

  log(label, "Waiting for token processing to complete");

  const modeButton = page.getByRole("button", { name: modeLabel });
  const startSyncButton = page.getByRole("button", { name: /Start Syncing/i });

  log(label, "Waiting for mode button to appear...");

  const syncStatusButton = page.getByLabel("Sync status");
  const popoverContent = page.locator('[data-slot="popover-content"]');

  async function isSynced(): Promise<boolean> {
    try {
      const expanded = await syncStatusButton.getAttribute("aria-expanded");
      if (expanded !== "true") {
        await syncStatusButton.click();
      }
    } catch { /* ignore */ }

    const popoverSynced = await popoverContent
      .filter({ hasText: "Synced" })
      .isVisible()
      .catch(() => false);
    if (popoverSynced) return true;

    const syncText = await syncStatusButton.textContent().catch(() => "");
    return syncText?.includes("Synced") ?? false;
  }

  try {
    await modeButton.waitFor({ state: "visible", timeout: 30000 });
    log(label, "Mode button is visible");
  } catch {
    const dialogVisible = await page.getByRole("dialog").isVisible().catch(() => false);
    if (!dialogVisible) {
      if (await isSynced()) {
        log(label, "Wizard closed, already synced");
        return;
      }
    }
    throw new Error(`${label}: Mode button "${modeLabel}" did not appear`);
  }

  await modeButton.click();
  log(label, `Init flow - clicked ${modeLabel}`);

  await startSyncButton.waitFor({ state: "visible", timeout: 15000 });
  await startSyncButton.click();
  log(label, "Clicked Start Syncing");

  log(label, "Waiting for sync to complete...");

  const syncingIndicator = page.getByText("Syncing...");
  try {
    await syncingIndicator.waitFor({ state: "visible", timeout: 5000 });
    log(label, "Sync started, waiting for completion...");
  } catch {
    log(label, "Syncing indicator not seen, sync may have completed quickly");
  }

  await page.waitForTimeout(500);
  await page.getByRole("dialog").waitFor({ state: "hidden", timeout: 30000 }).catch(() => {
    // Fallback: check sync status popover
  });

  const synced = await isSynced();
  if (!synced) {
    log(label, "Sync dialog closed but status not synced; waiting briefly");
    await page.waitForTimeout(2000);
  }
}

// =========================================================================
// Test Suite
// =========================================================================

test.describe.serial("Sync V2", () => {
  test.beforeAll(async ({}, testInfo) => {
    if (testInfo.project.name === "webkit") {
      return;
    }

    if (!process.env.SYNC_SERVER_URL) {
      serverPort = getProjectPort(testInfo.project.name);
      serverUrl = `http://${defaultServerHost}:${serverPort}`;
    }

    if (shouldStartServer) {
      if (!existsSync(syncServerBinary)) {
        log("server", `Binary not found at: ${syncServerBinary}`);
        log(
          "server",
          "Build server first: cargo build --release -p diaryx_sync_server",
        );
        throw new Error(
          "Sync server binary not found. Build with: cargo build --release -p diaryx_sync_server",
        );
      }

      tempDataDir = mkdtempSync(path.join(tmpdir(), "diaryx-sync-v2-test-"));
      log("server", `Using temp data dir: ${tempDataDir}`);

      log("server", `Starting server from binary: ${syncServerBinary}`);
      serverProcess = spawn(syncServerBinary, [], {
        cwd: repoRoot,
        stdio: "pipe",
        env: {
          ...process.env,
          DATABASE_PATH: path.join(tempDataDir, "test.db"),
          PORT: String(serverPort),
        },
      });

      serverProcess.stdout.on("data", (data: Buffer) => {
        log("server", data.toString().trim());
      });
      serverProcess.stderr.on("data", (data: Buffer) => {
        log("server-err", data.toString().trim());
      });

      serverProcess.on("error", (err: Error) => {
        log("server", `Process error: ${err.message}`);
      });

      serverProcess.on("exit", (code: number | null, signal: string | null) => {
        log("server", `Process exited with code ${code}, signal ${signal}`);
      });
    }

    serverAvailable = await waitForServerReady();
  });

  test.afterAll(async () => {
    if (serverProcess) {
      log("server", "Stopping server");
      serverProcess.kill("SIGINT");
      serverProcess = null;
    }

    if (tempDataDir && existsSync(tempDataDir)) {
      log("server", `Cleaning up temp dir: ${tempDataDir}`);
      rmSync(tempDataDir, { recursive: true, force: true });
      tempDataDir = null;
    }
  });

  // -----------------------------------------------------------------------
  // Test 1: No content duplication when second client joins
  // -----------------------------------------------------------------------
  test("no content duplication when second client joins", async ({ browser, browserName }) => {
    test.setTimeout(180000);
    test.skip(browserName === "webkit", "WebKit OPFS not fully supported");
    test.skip(!serverAvailable, "Sync server not available");

    const contextA = await browser.newContext();
    const contextB = await browser.newContext();
    const pageA = await contextA.newPage();
    const pageB = await contextB.newPage();

    setupConsoleLogs(pageA, "clientA");
    setupConsoleLogs(pageB, "clientB");

    await pageA.goto("/");
    await pageB.goto("/");

    await clearAllBrowserStorage(pageA);
    await clearAllBrowserStorage(pageB);
    await pageA.reload();
    await pageB.reload();

    await waitForAppReady(pageA, 40000);
    await waitForAppReady(pageB, 40000);
    await waitForWorkspaceCrdtInitialized(pageA);
    await waitForWorkspaceCrdtInitialized(pageB);
    await enableShowAllFiles(pageA);
    await enableShowAllFiles(pageB);

    const ts = Date.now();
    const testEmail = `sync-v2-dup-${ts}@example.com`;
    const uniqueBody = `Unique body content ${ts}`;

    log("test", "Creating entry on clientA");
    await createEntry(pageA, `dup-test-${ts}.md`, `Dup Test ${ts}`, uniqueBody);

    log("test", "Auth + sync clientA");
    await completeAuthAndInit(pageA, testEmail, /Sync local content/i, "clientA");
    await pageA.waitForTimeout(3000);

    log("test", "Auth + sync clientB");
    await completeAuthAndInit(pageB, testEmail, /Load from server/i, "clientB");

    log("test", "Waiting for file on clientB");
    await waitForFileExists(pageB, `dup-test-${ts}.md`, 30000);

    log("test", "Waiting for body content to arrive on clientB");
    await waitForEntryContent(pageB, `dup-test-${ts}.md`, uniqueBody, 30000);

    log("test", "Checking body content on clientB for duplication");
    const bodyB = await pageB.evaluate(async (args) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      const candidates = [args.path, `./${args.path}`];
      for (const candidate of candidates) {
        try {
          const entry = await api.getEntry(candidate);
          if (entry?.content) return entry.content;
        } catch { /* ignore */ }
        try {
          const content = await api.readFile(candidate);
          if (content) return content;
        } catch { /* ignore */ }
      }
      return "";
    }, { path: `dup-test-${ts}.md` });

    log("test", `clientB body content (${bodyB.length} chars): "${bodyB.slice(0, 200)}"`);

    // Count occurrences of the unique body string
    const occurrencesB = bodyB.split(uniqueBody).length - 1;
    log("test", `clientB body occurrences: ${occurrencesB}`);
    expect(occurrencesB).toBe(1);

    // Also verify clientA content is not duplicated
    log("test", "Checking body content on clientA for duplication");
    const bodyA = await pageA.evaluate(async (args) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      const candidates = [args.path, `./${args.path}`];
      for (const candidate of candidates) {
        try {
          const entry = await api.getEntry(candidate);
          if (entry?.content) return entry.content;
        } catch { /* ignore */ }
        try {
          const content = await api.readFile(candidate);
          if (content) return content;
        } catch { /* ignore */ }
      }
      return "";
    }, { path: `dup-test-${ts}.md` });

    log("test", `clientA body content (${bodyA.length} chars): "${bodyA.slice(0, 200)}"`);
    const occurrencesA = bodyA.split(uniqueBody).length - 1;
    log("test", `clientA body occurrences: ${occurrencesA}`);
    expect(occurrencesA).toBe(1);

    await contextA.close();
    await contextB.close();
  });

  // -----------------------------------------------------------------------
  // Test 1b: No content duplication after page reload
  // -----------------------------------------------------------------------
  test("no content duplication after page reload", async ({ browser, browserName }) => {
    test.setTimeout(180000);
    test.skip(browserName === "webkit", "WebKit OPFS not fully supported");
    test.skip(!serverAvailable, "Sync server not available");

    const context = await browser.newContext();
    const page = await context.newPage();

    setupConsoleLogs(page, "client");

    await page.goto("/");
    await clearAllBrowserStorage(page);
    await page.reload();

    await waitForAppReady(page, 40000);
    await waitForWorkspaceCrdtInitialized(page);
    await enableShowAllFiles(page);

    const ts = Date.now();
    const testEmail = `sync-v2-reload-${ts}@example.com`;
    const uniqueBody = `Reload test content ${ts}`;

    log("test", "Creating entry");
    await createEntry(page, `reload-test-${ts}.md`, `Reload Test ${ts}`, uniqueBody);

    log("test", "Auth + sync");
    await completeAuthAndInit(page, testEmail, /Sync local content/i, "client");
    await page.waitForTimeout(3000);

    log("test", "Verifying content before reload");
    await waitForEntryContent(page, `reload-test-${ts}.md`, uniqueBody, 15000);

    log("test", "Reloading page");
    await page.reload();
    // After reload, no entry is auto-selected so ProseMirror won't be visible.
    // Just wait for the body and CRDT init instead of waitForAppReady.
    await page.waitForSelector('body', { state: 'visible' });
    await waitForWorkspaceCrdtInitialized(page);

    // Wait for sync to re-establish after reload
    await page.waitForTimeout(5000);

    log("test", "Verifying content after reload");
    await waitForEntryContent(page, `reload-test-${ts}.md`, uniqueBody, 30000);

    const bodyAfterReload = await page.evaluate(async (args) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      const candidates = [args.path, `./${args.path}`];
      for (const candidate of candidates) {
        try {
          const entry = await api.getEntry(candidate);
          if (entry?.content) return entry.content;
        } catch { /* ignore */ }
        try {
          const content = await api.readFile(candidate);
          if (content) return content;
        } catch { /* ignore */ }
      }
      return "";
    }, { path: `reload-test-${ts}.md` });

    log("test", `Body after reload (${bodyAfterReload.length} chars): "${bodyAfterReload.slice(0, 200)}"`);

    const occurrences = bodyAfterReload.split(uniqueBody).length - 1;
    log("test", `Body occurrences after reload: ${occurrences}`);
    expect(occurrences).toBe(1);

    await context.close();
  });

  // -----------------------------------------------------------------------
  // Test 2: File creation propagates to second client after sync
  // -----------------------------------------------------------------------
  test("file creation propagates to second client after sync", async ({ browser, browserName }) => {
    test.setTimeout(180000);
    test.skip(browserName === "webkit", "WebKit OPFS not fully supported");
    test.skip(!serverAvailable, "Sync server not available");

    const contextA = await browser.newContext();
    const contextB = await browser.newContext();
    const pageA = await contextA.newPage();
    const pageB = await contextB.newPage();

    setupConsoleLogs(pageA, "clientA");
    setupConsoleLogs(pageB, "clientB");

    await pageA.goto("/");
    await pageB.goto("/");

    await clearAllBrowserStorage(pageA);
    await clearAllBrowserStorage(pageB);
    await pageA.reload();
    await pageB.reload();

    await waitForAppReady(pageA, 40000);
    await waitForAppReady(pageB, 40000);
    await waitForWorkspaceCrdtInitialized(pageA);
    await waitForWorkspaceCrdtInitialized(pageB);
    await enableShowAllFiles(pageA);
    await enableShowAllFiles(pageB);

    const ts = Date.now();
    const testEmail = `sync-v2-prop-${ts}@example.com`;

    log("test", "Auth + sync both clients");
    await completeAuthAndInit(pageA, testEmail, /Sync local content/i, "clientA");
    await pageA.waitForTimeout(2000);
    await completeAuthAndInit(pageB, testEmail, /Load from server/i, "clientB");
    await pageB.waitForTimeout(2000);

    log("test", "Creating new entry on clientA after both are synced");
    await createEntry(pageA, `new-${ts}.md`, `New File ${ts}`, `Body for new file ${ts}`);

    log("test", "Waiting for new file to appear on clientB");
    await waitForFileExists(pageB, `new-${ts}.md`, 30000);
    await waitForEntryContent(pageB, `new-${ts}.md`, `Body for new file ${ts}`);

    log("test", "Verifying tree item visible on clientB");
    await enableShowAllFiles(pageB);
    await expect(
      pageB.getByRole("treeitem", { name: new RegExp(`New File ${ts}`) }),
    ).toBeVisible({ timeout: 30000 });

    await contextA.close();
    await contextB.close();
  });

  // -----------------------------------------------------------------------
  // Test 3: Content persists after page refresh
  // -----------------------------------------------------------------------
  test("content persists after page refresh", async ({ browser, browserName }) => {
    test.setTimeout(180000);
    test.skip(browserName === "webkit", "WebKit OPFS not fully supported");
    test.skip(!serverAvailable, "Sync server not available");

    const contextA = await browser.newContext();
    const contextB = await browser.newContext();
    const pageA = await contextA.newPage();
    const pageB = await contextB.newPage();

    setupConsoleLogs(pageA, "clientA");
    setupConsoleLogs(pageB, "clientB");

    await pageA.goto("/");
    await pageB.goto("/");

    await clearAllBrowserStorage(pageA);
    await clearAllBrowserStorage(pageB);
    await pageA.reload();
    await pageB.reload();

    await waitForAppReady(pageA, 40000);
    await waitForAppReady(pageB, 40000);
    await waitForWorkspaceCrdtInitialized(pageA);
    await waitForWorkspaceCrdtInitialized(pageB);
    await enableShowAllFiles(pageA);
    await enableShowAllFiles(pageB);

    const ts = Date.now();
    const testEmail = `sync-v2-persist-${ts}@example.com`;

    log("test", "Creating initial entry on clientA");
    await createEntry(pageA, `persist-${ts}.md`, `Persist Test ${ts}`, `Original content ${ts}`);

    log("test", "Auth + sync clientA");
    await completeAuthAndInit(pageA, testEmail, /Sync local content/i, "clientA");
    await pageA.waitForTimeout(3000);

    log("test", "Auth + sync clientB");
    await completeAuthAndInit(pageB, testEmail, /Load from server/i, "clientB");

    log("test", "Waiting for content on clientB");
    await waitForEntryContent(pageB, `persist-${ts}.md`, `Original content ${ts}`);

    log("test", "Updating content on clientA");
    await pageA.evaluate(async (args) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      await api.saveEntry(args.path, args.body);
    }, { path: `persist-${ts}.md`, body: `Updated content ${ts}` });

    log("test", "Waiting for updated content on clientB");
    await waitForEntryContent(pageB, `persist-${ts}.md`, `Updated content ${ts}`, 30000);

    log("test", "Refreshing both pages");
    await pageA.reload();
    await pageB.reload();

    // After reload, no entry is auto-selected so ProseMirror won't be visible.
    // Just wait for the body and CRDT init instead of waitForAppReady.
    await pageA.waitForSelector('body', { state: 'visible' });
    await pageB.waitForSelector('body', { state: 'visible' });
    await waitForWorkspaceCrdtInitialized(pageA);
    await waitForWorkspaceCrdtInitialized(pageB);

    // Wait for sync to re-establish
    await pageA.waitForTimeout(5000);
    await pageB.waitForTimeout(5000);

    log("test", "Verifying content persists after refresh on clientA");
    await waitForEntryContent(pageA, `persist-${ts}.md`, `Updated content ${ts}`, 30000);

    log("test", "Verifying content persists after refresh on clientB");
    await waitForEntryContent(pageB, `persist-${ts}.md`, `Updated content ${ts}`, 30000);

    await contextA.close();
    await contextB.close();
  });

  // -----------------------------------------------------------------------
  // Test 4: Temporary files are not synced
  // -----------------------------------------------------------------------
  test("temporary files are not synced", async ({ browser, browserName }) => {
    test.setTimeout(180000);
    test.skip(browserName === "webkit", "WebKit OPFS not fully supported");
    test.skip(!serverAvailable, "Sync server not available");

    const contextA = await browser.newContext();
    const contextB = await browser.newContext();
    const pageA = await contextA.newPage();
    const pageB = await contextB.newPage();

    setupConsoleLogs(pageA, "clientA");
    setupConsoleLogs(pageB, "clientB");

    await pageA.goto("/");
    await pageB.goto("/");

    await clearAllBrowserStorage(pageA);
    await clearAllBrowserStorage(pageB);
    await pageA.reload();
    await pageB.reload();

    await waitForAppReady(pageA, 40000);
    await waitForAppReady(pageB, 40000);
    await waitForWorkspaceCrdtInitialized(pageA);
    await waitForWorkspaceCrdtInitialized(pageB);
    await enableShowAllFiles(pageA);
    await enableShowAllFiles(pageB);

    const ts = Date.now();
    const testEmail = `sync-v2-temp-${ts}@example.com`;

    log("test", "Auth + sync both clients");
    await completeAuthAndInit(pageA, testEmail, /Sync local content/i, "clientA");
    await pageA.waitForTimeout(2000);
    await completeAuthAndInit(pageB, testEmail, /Load from server/i, "clientB");
    await pageB.waitForTimeout(2000);

    log("test", "Creating real file on clientA");
    await createEntry(pageA, `real-${ts}.md`, `Real File ${ts}`, `Real body ${ts}`);

    log("test", "Attempting to create temp files on clientA");
    await pageA.evaluate(async (args) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      // Try to save temp files - these should be filtered
      for (const ext of [".tmp", ".swap", ".bak"]) {
        try {
          await api.saveEntry(`temp-${args.ts}${ext}`, `Temp content ${ext}`);
        } catch (e) {
          console.log(`[test] Expected: saveEntry for ${ext} may fail:`, e);
        }
      }
    }, { ts });

    log("test", "Waiting for real file on clientB (control)");
    await waitForFileExists(pageB, `real-${ts}.md`, 30000);

    // Wait extra time to give temp files a chance to sync (they shouldn't)
    await pageB.waitForTimeout(5000);

    log("test", "Verifying temp files did NOT sync to clientB");
    for (const ext of [".tmp", ".swap", ".bak"]) {
      await waitForFileMissing(pageB, `temp-${ts}${ext}`, 5000);
      log("test", `Confirmed: temp-${ts}${ext} is missing on clientB`);
    }

    log("test", "Checking clientA CRDT doesn't contain temp file entries");
    const crdtHasTempFiles = await pageA.evaluate(async (args) => {
      const { getAllFiles } = await import("/src/lib/crdt/workspaceCrdtBridge");
      const files = await getAllFiles();
      const tempFiles: string[] = [];
      for (const [filePath] of files.entries()) {
        if (filePath.includes(`temp-${args.ts}`)) {
          tempFiles.push(filePath);
        }
      }
      return tempFiles;
    }, { ts });

    log("test", `Temp files in CRDT: ${JSON.stringify(crdtHasTempFiles)}`);
    expect(crdtHasTempFiles).toHaveLength(0);

    await contextA.close();
    await contextB.close();
  });

  // -----------------------------------------------------------------------
  // Test 5: Client B edit after load-from-server does not overwrite Client A
  // -----------------------------------------------------------------------
  test("client B edit after load-from-server preserves client A content", async ({ browser, browserName }) => {
    test.setTimeout(180000);
    test.skip(browserName === "webkit", "WebKit OPFS not fully supported");
    test.skip(!serverAvailable, "Sync server not available");

    const contextA = await browser.newContext();
    const contextB = await browser.newContext();
    const pageA = await contextA.newPage();
    const pageB = await contextB.newPage();

    setupConsoleLogs(pageA, "clientA");
    setupConsoleLogs(pageB, "clientB");

    await pageA.goto("/");
    await pageB.goto("/");

    await clearAllBrowserStorage(pageA);
    await clearAllBrowserStorage(pageB);
    await pageA.reload();
    await pageB.reload();

    await waitForAppReady(pageA, 40000);
    await waitForAppReady(pageB, 40000);
    await waitForWorkspaceCrdtInitialized(pageA);
    await waitForWorkspaceCrdtInitialized(pageB);
    await enableShowAllFiles(pageA);
    await enableShowAllFiles(pageB);

    const ts = Date.now();
    const testEmail = `sync-v2-editprop-${ts}@example.com`;
    const originalBody = `Original content from client A ${ts}`;
    const appendedText = ` plus client B addition ${ts}`;

    log("test", "Creating entry on clientA");
    await createEntry(pageA, `editprop-${ts}.md`, `EditProp Test ${ts}`, originalBody);

    log("test", "Auth + sync clientA");
    await completeAuthAndInit(pageA, testEmail, /Sync local content/i, "clientA");
    await pageA.waitForTimeout(3000);

    log("test", "Auth + sync clientB (load from server)");
    await completeAuthAndInit(pageB, testEmail, /Load from server/i, "clientB");

    log("test", "Waiting for body content to arrive on clientB");
    await waitForEntryContent(pageB, `editprop-${ts}.md`, originalBody, 30000);

    log("test", "Client B appending text via saveEntry");
    await pageB.evaluate(async (args) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      const candidates = [args.path, `./${args.path}`];
      for (const candidate of candidates) {
        try {
          const entry = await api.getEntry(candidate);
          if (entry?.content) {
            await api.saveEntry(candidate, entry.content + args.appendedText);
            return;
          }
        } catch { /* ignore */ }
      }
      throw new Error("Could not find entry to update on clientB");
    }, { path: `editprop-${ts}.md`, appendedText });

    log("test", "Waiting for appended content on clientA");
    await waitForEntryContent(pageA, `editprop-${ts}.md`, appendedText, 30000);

    log("test", "Verifying clientA still has original content");
    const bodyA = await pageA.evaluate(async (args) => {
      const bridge = (globalThis as any).__diaryx_bridge;
      if (bridge?.ensureBodySync && bridge?.getBodyContentFromCrdt) {
        for (const candidate of [args.path, `./${args.path}`]) {
          try {
            await bridge.ensureBodySync(candidate);
            const content = await bridge.getBodyContentFromCrdt(candidate);
            if (content) return content;
          } catch { /* ignore */ }
        }
      }
      const { getBackend, createApi } = await import("/src/lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      for (const candidate of [args.path, `./${args.path}`]) {
        try {
          const entry = await api.getEntry(candidate);
          if (entry?.content) return entry.content;
        } catch { /* ignore */ }
      }
      return "";
    }, { path: `editprop-${ts}.md` });

    log("test", `clientA body (${bodyA.length} chars): "${bodyA.slice(0, 300)}"`);
    expect(bodyA).toContain(originalBody);
    expect(bodyA).toContain(appendedText);

    log("test", "Verifying clientB still has original content");
    const bodyB = await pageB.evaluate(async (args) => {
      const bridge = (globalThis as any).__diaryx_bridge;
      if (bridge?.ensureBodySync && bridge?.getBodyContentFromCrdt) {
        for (const candidate of [args.path, `./${args.path}`]) {
          try {
            await bridge.ensureBodySync(candidate);
            const content = await bridge.getBodyContentFromCrdt(candidate);
            if (content) return content;
          } catch { /* ignore */ }
        }
      }
      const { getBackend, createApi } = await import("/src/lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      for (const candidate of [args.path, `./${args.path}`]) {
        try {
          const entry = await api.getEntry(candidate);
          if (entry?.content) return entry.content;
        } catch { /* ignore */ }
      }
      return "";
    }, { path: `editprop-${ts}.md` });

    log("test", `clientB body (${bodyB.length} chars): "${bodyB.slice(0, 300)}"`);
    expect(bodyB).toContain(originalBody);
    expect(bodyB).toContain(appendedText);

    await contextA.close();
    await contextB.close();
  });
});
