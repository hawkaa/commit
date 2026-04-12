// Commit — Popup keyring management
// Displays public key, manages network contacts

import "./popup.css";

interface KeyringEntry {
  publicKeyHex: string;
  label: string;
  addedAt: string;
}

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
 * Render the keyring list from storage.
 */
async function renderKeyring(): Promise<void> {
  const listEl = document.getElementById("keyring-list")!;
  const stored = await chrome.storage.local.get("keyring");
  const keyring: KeyringEntry[] = stored.keyring || [];

  listEl.innerHTML = "";

  if (keyring.length === 0) {
    const empty = document.createElement("div");
    empty.className = "keyring-empty";
    empty.textContent = "No contacts yet";
    listEl.appendChild(empty);
    return;
  }

  for (const entry of keyring) {
    const row = document.createElement("div");
    row.className = "keyring-entry";

    const label = document.createElement("span");
    label.className = "keyring-label";
    label.textContent = entry.label;

    const key = document.createElement("span");
    key.className = "keyring-key";
    key.textContent = truncateKey(entry.publicKeyHex);
    key.title = entry.publicKeyHex;

    const removeBtn = document.createElement("button");
    removeBtn.className = "popup-btn popup-btn--danger";
    removeBtn.textContent = "Remove";
    removeBtn.addEventListener("click", async () => {
      await chrome.runtime.sendMessage({
        type: "KEYRING_REMOVE",
        publicKeyHex: entry.publicKeyHex,
      });
      await renderKeyring();
    });

    row.appendChild(label);
    row.appendChild(key);
    row.appendChild(removeBtn);
    listEl.appendChild(row);
  }
}

/**
 * Set up the "Add to network" form.
 */
function setupAddForm(): void {
  const keyInput = document.getElementById(
    "add-key-input"
  ) as HTMLInputElement;
  const labelInput = document.getElementById(
    "add-label-input"
  ) as HTMLInputElement;
  const addBtn = document.getElementById("add-key-btn")!;

  addBtn.addEventListener("click", async () => {
    const publicKeyHex = keyInput.value.trim().toLowerCase();
    const label = labelInput.value.trim() || "Unknown";

    // Validate: 64-char hex (32-byte Ed25519 public key)
    if (
      publicKeyHex.length !== 64 ||
      !/^[0-9a-f]+$/.test(publicKeyHex)
    ) {
      keyInput.style.borderColor = "#dc2626";
      setTimeout(() => {
        keyInput.style.borderColor = "";
      }, 2000);
      return;
    }

    const result = await chrome.runtime.sendMessage({
      type: "KEYRING_ADD",
      publicKeyHex,
      label,
    });

    if (result?.success) {
      keyInput.value = "";
      labelInput.value = "";
      await renderKeyring();
    }
  });
}

// Initialize popup
document.addEventListener("DOMContentLoaded", async () => {
  await displayOwnKey();
  await renderKeyring();
  setupAddForm();
});
