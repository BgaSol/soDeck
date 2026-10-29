import {BoundedExecClient, Cancellation, type ExecRequest} from '@sodeck/sdk/bounded-exec';

// This runs on a separate, disposable exec channel. A non-POSIX shell may
// reject it without terminating the user's interactive terminal.
export const shellProbeCommand = `printf 'SODECK_SHELL:%s\\n' "\${BASH_VERSION-}"; uname -s`;

export async function supportsShellIntegration(
  client: BoundedExecClient,
  target: Pick<ExecRequest, 'scope' | 'leaseId' | 'connectionGeneration' | 'operationId'>,
  cancellation: Cancellation,
): Promise<boolean> {
  try {
    const result = await client.run({...target, command: shellProbeCommand, cancellation,
      remoteTimeoutMs: 2000, deadlineMs: 3000, maxOutputBytes: 1024});
    return !cancellation.canceled && !result.cleanupPending && !result.truncated
      && result.exitCode === 0 && result.endedReason === null
      && /^SODECK_SHELL:\d+(?:\.\d+)+(?:[^\r\n]*)\r?\nLinux\r?\n?$/.test(String.fromCharCode(...result.stdout));
  } catch { return false; }
}
