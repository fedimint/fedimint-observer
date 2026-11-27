import { useState } from 'react';
import { writeStorage } from '../utils/storage';
import { pageSizeKey, storedPageSize, type PageSize } from '../utils/pageSize';


export function usePageSize(list: string, fallback: PageSize = 25) {
  const [size, setSize] = useState<PageSize>(() => storedPageSize(list, fallback));
  const choose = (next: PageSize) => {
    writeStorage(pageSizeKey(list), String(next));
    setSize(next);
  };
  return [size, choose] as const;
}
