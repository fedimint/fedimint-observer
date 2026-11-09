// Keep unobserved intervals as gaps, including isolated observed points.
export function chartSegments(values: (number | null)[]): { index: number; value: number }[][] {
  const segments: { index: number; value: number }[][] = [];
  let current: { index: number; value: number }[] = [];
  values.forEach((value, index) => {
    if (value === null || !Number.isFinite(value)) {
      current = [];
    } else {
      if (!current.length) segments.push(current);
      current.push({ index, value });
    }
  });
  return segments;
}
