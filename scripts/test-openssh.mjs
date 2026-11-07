// Independent OpenSSH interoperability, using only an isolated loopback daemon and test keys.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import net from "node:net";
import { spawn, spawnSync } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { cargoPackageVersion } from "./cargo-package.mjs";
import { testCurl } from "./network-tools.mjs";

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-openssh-"));
const sshd = process.env.SODECK_SSHD ?? (process.platform === "win32" ? path.join(process.env.WINDIR, "System32/OpenSSH/sshd.exe") : "/usr/sbin/sshd");
const quote = value => `"${value.replaceAll("\\", "/")}"`;
let daemon;
try {
  const socket = net.createServer(); await new Promise(resolve => socket.listen(0, "127.0.0.1", resolve));
  const port = socket.address().port; await new Promise(resolve => socket.close(resolve));
  for (const name of ["host", "user"]) {
    const result = spawnSync("ssh-keygen", ["-q", "-t", "ed25519", "-N", "", "-f", path.join(directory, name)], { windowsHide: true, encoding: "utf8" });
    if (result.status !== 0) throw new Error(result.stderr || "ssh-keygen failed");
  }
  const username = os.userInfo().username;
  fs.writeFileSync(path.join(directory, "sshd_config"), [
    `Port ${port}`, "ListenAddress 127.0.0.1", `HostKey ${quote(path.join(directory, "host"))}`,
    `PidFile ${quote(path.join(directory, "pid"))}`, `AuthorizedKeysFile ${quote(path.join(directory, "user.pub"))}`,
    "StrictModes no", "PasswordAuthentication no", "KbdInteractiveAuthentication no", "PubkeyAuthentication yes",
    "AllowTcpForwarding yes", `Subsystem sftp ${process.env.SODECK_SFTP_SERVER ? quote(process.env.SODECK_SFTP_SERVER) : 'internal-sftp'}`, `LogLevel ${process.env.SODECK_SSHD_LOG_LEVEL ?? 'ERROR'}`, ...(process.platform === "win32" ? [] : ["UsePAM no"]), "",
  ].join("\n"));
  let diagnostic = "";
  daemon = spawn(sshd, ["-D", "-e", "-f", path.join(directory, "sshd_config")], { windowsHide: true, stdio: ["ignore", "ignore", "pipe"] });
  daemon.stderr.on("data", data => { diagnostic += data; });
  let spawnError; daemon.on("error", error => { spawnError = error; });
  let ready = false;
  for (let i = 0; i < 50; i++) {
    await delay(100);
    if (spawnError || daemon.exitCode !== null) throw new Error(`OpenSSH startup failed: ${spawnError ?? diagnostic}`);
    ready = await new Promise(resolve => { const probe = net.connect({ host: "127.0.0.1", port }); probe.on("connect", () => { probe.destroy(); resolve(true); }); probe.on("error", () => resolve(false)); });
    if (ready) break;
  }
  if (!ready) throw new Error("OpenSSH did not listen within 5 seconds");
  const packagedIndex=process.argv.indexOf("--packaged");
  if (packagedIndex !== -1) {
    const executable=process.argv.slice(packagedIndex+1); if (!executable.length) throw new Error("--packaged requires the installed executable or launcher command");
    const config=path.join(directory,"validation.json");
    const remoteFile=path.join(directory,'native-file.txt');fs.writeFileSync(remoteFile,'sodeck-native-original\n');
    fs.writeFileSync(config,JSON.stringify({port,username,privateKeyPath:path.join(directory,"user"),remoteFile:remoteFile.replaceAll('\\','/')}));
    const result=await new Promise((resolve,reject)=>{
      const smoke=spawn(process.execPath,["scripts/smoke.mjs",...executable],{windowsHide:true,stdio:"inherit",env:{...process.env,SODECK_TEST_SSH_CONFIG:config}});
      smoke.on("error",reject);smoke.on("exit",resolve);
    });
    if (result!==0) throw new Error(`Packaged OpenSSH validation failed (${result}): ${diagnostic}`);
  } else {
    const terminal=process.argv.includes('--terminal');
    const files=process.argv.includes('--files');
    const network=process.argv.includes('--network');
    const uiOnly=(files&&process.argv.includes('--files-ui-only'))||(network&&process.argv.includes('--network-ui-only'));
    const env={ ...process.env, SODECK_TEST_SSH_PORT: String(port), SODECK_TEST_SSH_USER: username, SODECK_TEST_SSH_KEY: path.join(directory, "user") };
    if(network) env.SODECK_TEST_CURL=await testCurl();
    for(const plugin of [terminal?'terminal':null,files?'files':null,network?'network-proxy':null].filter(Boolean)){
      const output=path.join(directory,'plugins');
      const build=spawnSync(process.execPath,['scripts/package-plugin.mjs','--plugin',plugin,'--out-dir',output],{stdio:'inherit',windowsHide:true});if(build.status!==0)throw new Error(`${plugin} package failed`);
      const version=cargoPackageVersion(`plugins/${plugin}/Cargo.toml`);
      const target=spawnSync('rustc',['-vV'],{encoding:'utf8'}).stdout.match(/^host: (.+)$/m)[1];
      env[`SODECK_TEST_${(plugin==='network-proxy'?'NETWORK':plugin.toUpperCase())}_PLUGIN`]=path.join(output,`sodeck-${plugin}_${version}_${target}`);
    }
      const result = uiOnly ? 0 : await new Promise((resolve, reject) => {
    const test = spawn("cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml", "--features", "test-fixtures", "--locked", "--test", "connections", network?'network_proxy_plugin':files?'openssh_files_plugin':terminal?'openssh_terminal_plugin':'openssh_interop', "--", "--ignored", "--exact", "--nocapture"], {
      windowsHide: true, stdio: "inherit", env,
    }); test.on("error", reject); test.on("exit", resolve);
  });
  if (result !== 0) throw new Error(`OpenSSH interoperability failed (${result}): ${diagnostic}`);
  if(network&&!uiOnly){const result=await new Promise((resolve,reject)=>{const test=spawn('cargo',['test','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--test','connections','openssh_network_plugin','--','--ignored','--exact','--nocapture'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('OpenSSH network plugin failed: '+diagnostic);}
  if(network&&!uiOnly){for(const name of ['network_socks_protocol','network_socks_lifecycle']){const result=await new Promise((resolve,reject)=>{const test=spawn('cargo',['test','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--test','connections',name,'--','--ignored','--exact','--nocapture'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error(name+' failed');}}
  if(network){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-network-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('Network UI failed');}
  if(terminal){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-terminal-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('Terminal UI failed');}
  if(files){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-files-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('File UI failed');}
  }
} finally {
  if (daemon && daemon.exitCode === null) { const exited = new Promise(resolve => daemon.once("exit", resolve)); daemon.kill(); await exited; }
  if (path.dirname(path.resolve(directory)) !== path.resolve(os.tmpdir()) || !path.basename(directory).startsWith("sodeck-openssh-")) throw new Error("Unexpected test cleanup path");
  fs.rmSync(directory, { recursive: true, force: true });
}
