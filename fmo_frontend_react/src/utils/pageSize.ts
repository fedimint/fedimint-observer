
import { readStorage } from './storage.ts';

export const PAGE_SIZES = [10, 25, 50, 100] as const;
export type PageSize = (typeof PAGE_SIZES)[number];

export const pageSizeKey = (list: string) => `pageSize:${list}`;

/** The size last chosen for a list on this device, else `fallback` */
export function storedPageSize(list: string, fallback: PageSize = 25): PageSize {
  return parsePageSize(readStorage(pageSizeKey(list))) ?? fallback;
}

/** A stored or URL value, if it is one of the offered sizes */
export function parsePageSize(value: string | null | undefined): PageSize | null {
  const size = Number(value);
  return (PAGE_SIZES as readonly number[]).includes(size) ? (size as PageSize) : null;
}
