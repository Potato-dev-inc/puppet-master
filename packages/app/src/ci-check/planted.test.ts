// Planted failure: proves CI turns red. Reverted in the next commit.
import { expect, test } from 'vitest';
test('planted failure for the CI check', () => {
  expect(1).toBe(2);
});
