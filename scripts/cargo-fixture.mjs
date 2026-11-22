import path from 'node:path';

export function fixtureBinary(name) {
  return path.resolve(process.env.CARGO_TARGET_DIR || 'src-tauri/target', 'debug', name + (process.platform === 'win32' ? '.exe' : ''));
}
