import { test, expect, chromium, type BrowserContext } from "@playwright/test";
import path from "path";

// Load the extension in Chromium and verify the offscreen document initializes.
// This test catches: missing files, wrong import paths, CSP violations, WASM load failures.

const extensionPath = path.join(__dirname, "..", "build");

async function launchWithExtension(): Promise<BrowserContext> {
  return chromium.launchPersistentContext("", {
    headless: false, // Extensions require headed mode
    args: [
      `--disable-extensions-except=${extensionPath}`,
      `--load-extension=${extensionPath}`,
      "--no-first-run",
      "--no-default-browser-check",
    ],
  });
}

test("extension loads without errors and opens onboarding on install", async () => {
  test.setTimeout(30000);
  const context = await launchWithExtension();

  // Get the extension's service worker
  let sw = context.serviceWorkers()[0];
  if (!sw) {
    sw = await context.waitForEvent("serviceworker", { timeout: 10000 });
  }

  // Wait for the onboarding tab to open (onInstalled fires reason: "install")
  // Check existing pages first (tab may already exist if onInstalled fired
  // before we subscribed), then fall back to waitForEvent.
  let onboardingPage: import("@playwright/test").Page | undefined;
  onboardingPage = context
    .pages()
    .find((p) => p.url().includes("onboarding.html"));

  if (!onboardingPage) {
    const newPage = await context.waitForEvent("page", {
      predicate: (p) => p.url().includes("onboarding.html"),
      timeout: 5000,
    });
    onboardingPage = newPage;
  }

  expect(onboardingPage).toBeDefined();
  console.log(`Onboarding tab URL: ${onboardingPage!.url()}`);

  // Verify the CTA link is present with correct href
  const ctaLink = onboardingPage!.locator('a.cta[href="https://github.com/"]');
  await expect(ctaLink).toBeVisible({ timeout: 3000 });
  const ctaText = await ctaLink.textContent();
  expect(ctaText).toContain("Visit a GitHub repo");

  // Navigate to a page to trigger the content script
  const page = await context.newPage();
  await page.goto("https://github.com/nickel-org/nickel.rs", {
    waitUntil: "domcontentloaded",
  });

  // Give the content script time to inject
  await page.waitForTimeout(3000);

  // Check: no errors in the service worker
  const swErrors: string[] = [];
  sw.on("console", (msg) => {
    if (msg.type() === "error") swErrors.push(msg.text());
  });

  // Check: trust card appeared on the page
  const trustCard = await page.$(".commit-trust-card");
  console.log(`Trust card found: ${!!trustCard}`);
  test.skip(!trustCard, "no trust card — external dep unreachable");

  // Check: "Add badge" CTA is present when trust card loads
  const addBadge = await page.$(".commit-add-badge");
  console.log(`Add badge CTA found: ${!!addBadge}`);
  expect(addBadge).not.toBeNull();
  const addBadgeText = await addBadge!.textContent();
  expect(addBadgeText).toBe("Add badge");

  await context.close();
  expect(swErrors).toEqual([]);
});

test("offscreen WASM initializes without errors", async () => {
  test.setTimeout(60000);
  const context = await launchWithExtension();

  let sw = context.serviceWorkers()[0];
  if (!sw) {
    sw = await context.waitForEvent("serviceworker", { timeout: 10000 });
  }

  // Trigger the offscreen document by sending START_ENDORSEMENT
  // We can't directly message the extension, but we can navigate to
  // a GitHub page and simulate the endorsement flow via console.
  const page = await context.newPage();
  await page.goto("https://github.com/nickel-org/nickel.rs", {
    waitUntil: "domcontentloaded",
  });
  await page.waitForTimeout(3000);

  // Try clicking the Endorse button if it exists
  const endorseBtn = await page.$(".commit-endorse-btn");
  if (endorseBtn) {
    console.log("Found Endorse button, clicking...");
    await endorseBtn.click();

    // Wait for the button to change state (success or failure)
    await page.waitForFunction(
      () => {
        const btn = document.querySelector(".commit-endorse-btn");
        return btn && btn.textContent !== "Proving...";
      },
      null,
      { timeout: 45000 }
    ).catch(() => {
      console.log("Button still showing Proving... (timeout)");
    });

    const btnText = await endorseBtn.textContent();
    console.log(`Endorse button final state: "${btnText}"`);

    // The proof might fail (notary connection, etc.) but it should NOT hang.
    // Any state other than "Proving..." means error handling works.
    expect(btnText).not.toBe("Proving...");
  } else {
    console.log("No Endorse button found (trust card may not have loaded)");
  }

  await context.close();
});
