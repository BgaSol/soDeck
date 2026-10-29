import {describe, it, expect} from 'vitest';
import {checkedClock, workerClock} from './runtime';
describe('public monotonic worker clock', () => {
  it('rejects missing, nonfinite and backwards clocks', () => {
    expect(() => workerClock({})).toThrow('runtime.clock');
    let now = 3; const clock = checkedClock({monotonicMs: () => now});
    expect(clock.monotonicMs()).toBe(3); now = 2; expect(() => clock.monotonicMs()).toThrow();
    expect(() => checkedClock({monotonicMs: () => NaN}).monotonicMs()).toThrow();
  });
});
