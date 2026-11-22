export interface PluginError { code: string | number; message: string; data?: unknown }

export function surfaceError(error: unknown): PluginError {
  if (error && typeof error === 'object' && 'message' in error) {
    const value = error as {code?: unknown; message: unknown; data?: unknown};
    return {code: typeof value.code === 'string' || typeof value.code === 'number' ? value.code : 'hostError', message: String(value.message), data: value.data};
  }
  return {code: 'hostError', message: String(error)};
}
