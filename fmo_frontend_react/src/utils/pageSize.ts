/** Page sizes offered on every paged list; the backend accepts 1 to 500 */
export const PAGE_SIZES = [10, 25, 50, 100] as const;
export type PageSize = (typeof PAGE_SIZES)[number];

/** A stored or URL value, if it is one of the offered sizes */
export function parsePageSize(value: string | null | undefined): PageSize | null {
  const size = Number(value);
  return (PAGE_SIZES as readonly number[]).includes(size) ? (size as PageSize) : null;
}
