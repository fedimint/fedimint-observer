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
