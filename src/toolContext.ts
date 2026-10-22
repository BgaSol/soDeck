/** A tool capability has its own identity and generation. Never pass a PTY id. */
export interface ToolContextRef {
  id: string;
  epoch: number;
}

// Stores use the opaque capability id as their stable key. The generation is
// encoded by the backend as the final component; IPC always sends the full ref.
export function contextReference(contextId: string): ToolContextRef {
  const epoch = Number(contextId.slice(contextId.lastIndexOf(".") + 1));
  if (!Number.isSafeInteger(epoch) || epoch < 1 || !contextId.includes("."))
    throw new Error("服务器工具连接已失效，请重新验证工具连接。");
  return { id: contextId, epoch };
}
