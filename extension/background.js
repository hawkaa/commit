// Commit — Background service worker
// Manages keypair, extension-side cache cleanup

chrome.runtime.onInstalled.addListener(async () => {
  // Generate ed25519 keypair on first install
  const existing = await chrome.storage.local.get("keypair");
  if (!existing.keypair) {
    const keyPair = await crypto.subtle.generateKey(
      { name: "Ed25519" },
      true,
      ["sign", "verify"]
    );
    const publicKey = await crypto.subtle.exportKey("raw", keyPair.publicKey);
    const privateKey = await crypto.subtle.exportKey("pkcs8", keyPair.privateKey);

    await chrome.storage.local.set({
      keypair: {
        publicKey: Array.from(new Uint8Array(publicKey)),
        privateKey: Array.from(new Uint8Array(privateKey)),
        createdAt: new Date().toISOString(),
      },
    });
    console.log("[commit] Keypair generated on install");
  }
});

// Periodic cache cleanup (every 6 hours)
chrome.alarms.create("cache-cleanup", { periodInMinutes: 360 });
chrome.alarms.onAlarm.addListener(async (alarm) => {
  if (alarm.name !== "cache-cleanup") return;

  const all = await chrome.storage.local.get(null);
  const now = Date.now();
  const ttl = 60 * 60 * 1000; // 1 hour

  for (const [key, value] of Object.entries(all)) {
    if (key.startsWith("trust-card:") && value.timestamp && now - value.timestamp > ttl) {
      await chrome.storage.local.remove(key);
    }
  }
});
