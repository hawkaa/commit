// Commit — Popup status card
// Displays public key, endorsement count, and about link

import "./popup.css";

/**
 * Hex-encode a byte array.
 */
function bytesToHex(bytes: number[]): string {
  return bytes.map((b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Truncate a hex key for display: first 8 + last 8 chars.
 */
function truncateKey(hex: string): string {
  if (hex.length <= 20) return hex;
  return `${hex.slice(0, 8)}...${hex.slice(-8)}`;
}

/**
 * Load and display the user's own public key.
 */
async function displayOwnKey(): Promise<void> {
  const keyEl = document.getElementById("public-key")!;
  const copyBtn = document.getElementById("copy-key")!;

  const stored = await chrome.storage.local.get("keypair");
  if (!stored.keypair?.publicKey) {
    keyEl.textContent = "No keypair found";
    return;
  }

  const fullHex = bytesToHex(stored.keypair.publicKey);
  let expanded = false;

  keyEl.textContent = truncateKey(fullHex);
  keyEl.title = "Click to expand/collapse";

  keyEl.addEventListener("click", () => {
    expanded = !expanded;
    keyEl.textContent = expanded ? fullHex : truncateKey(fullHex);
    keyEl.classList.toggle("expanded", expanded);
  });

  copyBtn.addEventListener("click", async () => {
    await navigator.clipboard.writeText(fullHex);
    copyBtn.textContent = "Copied!";
    setTimeout(() => {
      copyBtn.textContent = "Copy";
    }, 1500);
  });
}

/**
 * Load and display the local endorsement count.
 * Reads the counter incremented by background.ts after each successful endorsement.
 */
async function displayEndorsementCount(): Promise<void> {
  const countEl = document.getElementById("endorsement-count")!;
  const labelEl = document.getElementById("endorsement-label")!;

  const { endorsement_count = 0 } = await chrome.storage.local.get("endorsement_count");
  const count = endorsement_count as number;

  countEl.textContent = String(count);
  labelEl.textContent = count === 1 ? "endorsement made" : "endorsements made";
}

// Initialize popup
document.addEventListener("DOMContentLoaded", async () => {
  await displayOwnKey();
  await displayEndorsementCount();
});
