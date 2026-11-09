export const MSATS_PER_BTC = 100_000_000_000;

// Convert millisatoshis to Bitcoin number only (no BTC suffix)
export function toBitcoin(msats: number, decimals: number = 6): string {
  return (msats / MSATS_PER_BTC).toFixed(decimals);
}

// Convert millisatoshis to Bitcoin with specified decimal places
export function asBitcoin(msats: number, decimals: number = 6): string {
  return `${toBitcoin(msats, decimals)} BTC`;
}

// Format numbers with thousand separators
export function formatNumber(num: number): string {
  return num.toLocaleString('en-US');
}

// Shortens a long id in the middle; the full value belongs in a title
export function shortMiddle(value: string, head: number, tail: number, ellipsis: string = '…'): string {
  return value.length <= head + tail + ellipsis.length ? value : `${value.slice(0, head)}${ellipsis}${value.slice(-tail)}`;
}

// A millisatoshi amount in the largest unit that keeps it short: sats, k sats, M sats or BTC
export function formatMsatsCompact(msat: number): string {
  const sats = msat / 1000;
  if (sats >= 100_000_000) return `${(sats / 100_000_000).toFixed(2)} BTC`;
  if (sats >= 1_000_000) return `${(sats / 1_000_000).toFixed(1)}M sats`;
  if (sats >= 1_000) return `${(sats / 1_000).toFixed(1)}k sats`;
  return `${Math.round(sats).toLocaleString('en-US')} sats`;
}

// A millisatoshi amount in sats, keeping up to three decimals for sub-sat fees
export function formatMsatsAsSats(msat: number): string {
  return `${(msat / 1000).toLocaleString('en-US', { maximumFractionDigits: 3 })} sat`;
}
