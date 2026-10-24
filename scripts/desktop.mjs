import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, delimiter } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const cli = join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
const localPerl = join(root, '.local/build-tools/strawberry/perl/bin/perl.exe');
const child = spawn(process.execPath, [cli, ...process.argv.slice(2)], {
  cwd: join(root, 'apps', 'desktop'), stdio: 'inherit', windowsHide: true,
  env: { ...process.env, ...(existsSync(localPerl) ? {OPENSSL_SRC_PERL:process.env.OPENSSL_SRC_PERL ?? localPerl} : {}), PATH: [join(homedir(), '.cargo', 'bin'), process.env.PATH ?? ''].join(delimiter) },
});
child.on('error', error => { console.error(error.message); process.exitCode = 1; });
child.on('exit', code => { process.exitCode = code ?? 1; });
