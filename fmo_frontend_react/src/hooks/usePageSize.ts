import { useState } from 'react';
import { readStorage, writeStorage } from '../utils/storage';
import { parsePageSize, type PageSize } from '../utils/pageSize';

/**
 * How many rows a list asks the backend for per page, remembered per list in
 * this browser; storage can fail, and then the default applies.
 */
export function usePageSize(list: string, fallback: PageSize = 25) {
  const key = `pageSize:${list}`;
  const [size, setSize] = useState<PageSize>(() => parsePageSize(readStorage(key)) ?? fallback);
  const choose = (next: PageSize) => {
    writeStorage(key, String(next));
    setSize(next);
  };
  return [size, choose] as const;
}
