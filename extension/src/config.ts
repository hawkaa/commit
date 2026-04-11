// Shared configuration constants for the Commit extension.
// Single source of truth for URLs that appear in multiple entry points.

export const API_BASE = "https://commit-backend.fly.dev";
export const NOTARY_URL = "https://notary.pse.dev/v0.1.0-alpha.12";
export const PROXY_BASE = "wss://notary.pse.dev/proxy";
export const CACHE_TTL_MS = 60 * 60 * 1000; // 1 hour
