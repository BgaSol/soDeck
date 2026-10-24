import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, delimiter } from 'node:path';

const cargoDir = join(homedir(), '.cargo', 'bin');
const cargo = join(cargoDir, process.platform === 'win32' ? 'cargo.exe' : 'cargo');
const localPerl = join(import.meta.dirname, '../.local/build-tools/strawberry/perl/bin/perl.exe');
const child = spawn(existsSync(cargo) ? cargo : 'cargo', process.argv.slice(2), {
  stdio: 'inherit', windowsHide: true,
  env: { ...process.env, ...(existsSync(localPerl) ? {OPENSSL_SRC_PERL:process.env.OPENSSL_SRC_PERL ?? localPerl} : {}), PATH: [cargoDir, process.env.PATH ?? ''].join(delimiter) },
});
child.on('error', error => { console.error(error.message); process.exitCode = 1; });
child.on('exit', code => { process.exitCode = code ?? 1; });
