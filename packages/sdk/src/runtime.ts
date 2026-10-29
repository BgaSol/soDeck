/** Clock origins belong to a single runtime. Never compare worker and WebView clocks. */
export interface Clock { monotonicMs(): number }
export function checkedClock(source: Clock): Clock {
  let previous = -1;
  return {monotonicMs() {
    const now = source.monotonicMs();
    if (!Number.isFinite(now) || now < 0 || now < previous)
      throw Object.assign(new Error('运行时单调时钟无效'), {code: 'RUNTIME_CLOCK_INVALID'});
    previous = now;
    return now;
  }};
}
export function workerClock(runtime: {clock?: Clock}): Clock {
  if (typeof runtime.clock?.monotonicMs !== 'function')
    throw Object.assign(new Error('资源监控需要 runtime.clock.v1'), {code: 'UNSUPPORTED_API'});
  return checkedClock(runtime.clock);
}
