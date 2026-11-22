// Deterministic invariant fields for the Docker and MCP process RPC fixtures.
export function processFacts(value = {}) {
  const finished = value.finished ?? value.workerFinished ?? ['completed', 'cancelled', 'failed'].includes(value.state);
  return {
    state: 'opening', phase: finished ? 'finished' : 'running', execSent: true,
    launchQueued: true, launchAccepted: true, remoteExitConfirmed: false,
    exitCode: null, exitSignal: null, error: null, finished, workerFinished: finished,
    outputComplete: false, elapsedMs: 0, inputClosed: finished,
    input: {state: 'closed', available: false, nextInputSequence: '1', last: null},
    producedBytes: '0', earliestAvailableOffset: '0', ...value,
  };
}
export function withProcessFacts(handler) {
  return async (method, params) => {
    const result = await handler(method, params);
    if (!method.startsWith('session.process.')) return result;
    if (result.status) return {...result, status: processFacts(result.status)};
    if (['session.process.status', 'session.process.input', 'session.process.cancel'].includes(method)) return processFacts(result);
    return result;
  };
}
