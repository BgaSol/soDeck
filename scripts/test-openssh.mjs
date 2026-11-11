// Independent OpenSSH interoperability, using only an isolated loopback daemon and test keys.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import net from "node:net";
import { spawn, spawnSync } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { testPluginDirectory } from "./test-plugin-directory.mjs";
import { testCurl } from "./network-tools.mjs";
import { parseOpenSshTestArgs } from "./test-cli-arguments.mjs";

const { flags, launcher } = parseOpenSshTestArgs(process.argv.slice(2));
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
  if (launcher) {
    const config=path.join(directory,"validation.json");
    const remoteFile=path.join(directory,'native-file.txt');fs.writeFileSync(remoteFile,'sodeck-native-original\n');
    fs.writeFileSync(config,JSON.stringify({port,username,privateKeyPath:path.join(directory,"user"),remoteFile:remoteFile.replaceAll('\\','/')}));
    const result=await new Promise((resolve,reject)=>{
      const smoke=spawn(process.execPath,[flags['file-editor-native']?"scripts/test-file-editor-native.mjs":flags['close-native']?"scripts/test-close-native.mjs":"scripts/smoke.mjs",...launcher],{windowsHide:true,stdio:"inherit",env:{...process.env,SODECK_TEST_SSH_CONFIG:config}});
      smoke.on("error",reject);smoke.on("exit",resolve);
    });
    if (result!==0) throw new Error(`Packaged OpenSSH validation failed (${result}): ${diagnostic}`);
  } else {
    const communication=flags.communication;
    const terminal=flags.terminal||communication;
    const files=flags.files||communication;
    const network=flags.network;
    const status=flags['system-status']||communication;
    const uiOnly=communication||(files&&flags['files-ui-only'])||(network&&flags['network-ui-only']);
    const env={ ...process.env, SODECK_TEST_SSH_PORT: String(port), SODECK_TEST_SSH_USER: username, SODECK_TEST_SSH_KEY: path.join(directory, "user") };
    if(network) env.SODECK_TEST_CURL=await testCurl();
    for(const plugin of [terminal?'terminal':null,files?'files':null,network?'network-proxy':null,status?'system-status':null].filter(Boolean)){
      env[`SODECK_TEST_${(plugin==='network-proxy'?'NETWORK':plugin.toUpperCase().replaceAll('-','_'))}_PLUGIN`]=testPluginDirectory(plugin);
    }
    if(files){const root=env.SODECK_TEST_FILES_PLUGIN;const manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json'),'utf8'));const test=spawnSync(process.execPath,['scripts/test-files-process.mjs'],{stdio:'inherit',windowsHide:true,env:{...env,SODECK_TEST_FILES_EXECUTABLE:path.join(root,manifest.entry)}});if(test.status!==0)throw new Error('File process regressions failed');}
    if(network){const root=env.SODECK_TEST_NETWORK_PLUGIN;const manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json'),'utf8'));const test=spawnSync(process.execPath,['scripts/test-network-process.mjs'],{stdio:'inherit',windowsHide:true,env:{...env,SODECK_TEST_NETWORK_EXECUTABLE:path.join(root,manifest.entry)}});if(test.status!==0)throw new Error('Network process regressions failed');}
      const result = uiOnly ? 0 : await new Promise((resolve, reject) => {
    const test = spawn("cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml", "--features", "test-fixtures", "--locked", "--test", "connections", status?'openssh_system_status_plugin':network?'network_proxy_plugin':files?'openssh_files_plugin':terminal?'openssh_terminal_plugin':'openssh_interop', "--", "--ignored", "--exact", "--nocapture"], {
      windowsHide: true, stdio: "inherit", env,
    }); test.on("error", reject); test.on("exit", resolve);
  });
  if (result !== 0) throw new Error(`OpenSSH interoperability failed (${result}): ${diagnostic}`);
  if(network&&!uiOnly){const result=await new Promise((resolve,reject)=>{const test=spawn('cargo',['test','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--test','connections','openssh_network_plugin','--','--ignored','--exact','--nocapture'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('OpenSSH network plugin failed: '+diagnostic);}
  if(network&&!uiOnly){for(const name of ['network_socks_protocol','network_socks_lifecycle']){const result=await new Promise((resolve,reject)=>{const test=spawn('cargo',['test','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--test','connections',name,'--','--ignored','--exact','--nocapture'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error(name+' failed');}}
  if(network){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-network-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('Network UI failed');}
  if(status&&!communication){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-system-status-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('Status UI failed');}
  if(terminal&&!communication){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-terminal-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('Terminal UI failed');}
  if(files&&!communication){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-files-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('File UI failed');}
  if(communication){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-component-communication.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('Component communication failed');}
  }
} finally {
  if (daemon && daemon.exitCode === null) { const exited = new Promise(resolve => daemon.once("exit", resolve)); daemon.kill(); await exited; }
  if (path.dirname(path.resolve(directory)) !== path.resolve(os.tmpdir()) || !path.basename(directory).startsWith("sodeck-openssh-")) throw new Error("Unexpected test cleanup path");
  fs.rmSync(directory, { recursive: true, force: true });
}
