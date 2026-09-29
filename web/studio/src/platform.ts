export function isMac(): boolean {
  if (typeof navigator === "undefined") return false;
  // `navigator.platform` is deprecated but still the simplest check and
  // still populated by every browser that matters for local dev tooling
  // like this; userAgentData is not yet universal enough to rely on alone.
  return /Mac|iPhone|iPod|iPad/.test(navigator.platform ?? navigator.userAgent);
}
