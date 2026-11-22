import assert from 'node:assert/strict';

// Admission is asynchronous; source integration tests wait on the original receipt.
export async function completedAction(invoke, {window, context, requestId}, receipt) {
  if(Object.hasOwn(receipt,'result')) {assert(!receipt.error,JSON.stringify(receipt));receipt=receipt.result;}
  const deadline = Date.now() + 15000;
  while (receipt.status === 'pending') {
    assert(Date.now() < deadline, 'Command did not finish: ' + JSON.stringify(receipt));
    await new Promise(resolve => setTimeout(resolve, 25));
    const reply = await invoke('component_ui', { ...(window === undefined ? {} : {window}), operation: 'command', args: {op:'get',expectedContext:context, requestId} });
    assert(!reply.error,JSON.stringify(reply));receipt=reply.result;
  }
  assert.equal(receipt.status, 'completed', JSON.stringify(receipt));
  return receipt;
}
