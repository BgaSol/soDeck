import assert from 'node:assert/strict';

// Admission is asynchronous; source integration tests wait on the original receipt.
export async function completedAction(invoke, {window, context, requestId}, receipt) {
  const deadline = Date.now() + 15000;
  while (receipt.status === 'pending') {
    assert(Date.now() < deadline, 'Command did not finish: ' + JSON.stringify(receipt));
    await new Promise(resolve => setTimeout(resolve, 25));
    receipt = await invoke('component_ui', { ...(window === undefined ? {} : {window}), operation: 'actionStatus', args: {context, requestId} });
  }
  assert.equal(receipt.status, 'completed', JSON.stringify(receipt));
  return receipt;
}
