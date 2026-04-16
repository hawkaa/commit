// Endorsed-subjects cache — persists endorsement state in chrome.storage.local
// for "You endorsed this" revisit indicators on trust cards.

import { ENDORSED_CACHE_KEY } from "./config";

export type Sentiment = "positive" | "negative";

export interface EndorsedEntry {
  sentiment: Sentiment;
  timestamp: number;
}

type EndorsedMap = Record<string, EndorsedEntry>;

function cacheKey(kind: string, subjectId: string): string {
  return `${kind}:${subjectId}`;
}

/**
 * Look up a cached endorsement for the given subject.
 * Returns null on cache miss or if the storage read fails.
 */
export async function getEndorsement(
  kind: string,
  subjectId: string
): Promise<EndorsedEntry | null> {
  try {
    const stored = await chrome.storage.local.get(ENDORSED_CACHE_KEY);
    const map = stored[ENDORSED_CACHE_KEY] as EndorsedMap | undefined;
    if (!map || typeof map !== "object") return null;
    const entry = map[cacheKey(kind, subjectId)];
    if (
      !entry ||
      typeof entry.sentiment !== "string" ||
      typeof entry.timestamp !== "number"
    ) {
      return null;
    }
    return entry;
  } catch {
    return null;
  }
}

/**
 * Write (or overwrite) a cached endorsement entry for the given subject.
 * Failures are logged but never thrown — cache writes must not block the caller.
 */
export async function setEndorsement(
  kind: string,
  subjectId: string,
  sentiment: Sentiment
): Promise<void> {
  try {
    const stored = await chrome.storage.local.get(ENDORSED_CACHE_KEY);
    const map: EndorsedMap =
      stored[ENDORSED_CACHE_KEY] && typeof stored[ENDORSED_CACHE_KEY] === "object"
        ? (stored[ENDORSED_CACHE_KEY] as EndorsedMap)
        : {};
    map[cacheKey(kind, subjectId)] = { sentiment, timestamp: Date.now() };
    await chrome.storage.local.set({ [ENDORSED_CACHE_KEY]: map });
  } catch (err) {
    console.warn("[commit] endorsed-cache write failed", err);
  }
}

/**
 * Clear all cached endorsement entries. Useful for testing and future reset flows.
 */
export async function clearAll(): Promise<void> {
  try {
    await chrome.storage.local.remove(ENDORSED_CACHE_KEY);
  } catch (err) {
    console.warn("[commit] endorsed-cache clear failed", err);
  }
}
