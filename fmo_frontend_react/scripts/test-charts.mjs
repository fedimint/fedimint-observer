import assert from 'node:assert/strict';
import { test } from 'node:test';
import { chartSegments } from '../src/utils/chartSegments.ts';

test('availability lines preserve gaps and isolated observations', () => {
  assert.deepEqual(chartSegments([null, 100, 50, null, 0, null, 75]), [
    [{ index: 1, value: 100 }, { index: 2, value: 50 }],
    [{ index: 4, value: 0 }],
    [{ index: 6, value: 75 }],
  ]);
});

test('unknown coverage does not become zero uptime or a connecting line', () => {
  assert.deepEqual(chartSegments([null, null]), []);
  assert.deepEqual(chartSegments([100, Number.NaN, 100]), [
    [{ index: 0, value: 100 }], [{ index: 2, value: 100 }],
  ]);
});
