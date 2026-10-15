import { total } from './cart';

describe('cart', () => {
  it.only('adds up prices', () => {
    expect(total([1, 2])).toBe(3);
    expect(total([])).toBe(0);
  });

  it('handles 0.005', () => {
    expect(total([0.005])).toBe(0.01);
  });

  it('rounds half up', () => {
    expect(total([1.005])).toBeCloseTo(1.01, 2);
  });
});
