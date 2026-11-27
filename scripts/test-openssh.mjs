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
const extraDaemons=[];
const sudoTest = flags.mcp && process.platform === 'linux';
const sudoUser = `sdmcp${process.pid}`;
const sudoPassword = 'SoDeck-fixture-137';
const sudoers = `/etc/sudoers.d/${sudoUser}`;
let sudoAccount = false;
function privileged(args, input) {
  const result = spawnSync('sudo', ['-n', ...args], { input, encoding:'utf8', windowsHide:true });
  if (result.status !== 0) throw new Error(result.stderr || `Fixture setup failed: ${args[0]}`);
}
function startDaemon(config) {
  return spawn(sudoTest ? 'sudo' : sshd, sudoTest ? ['-n', sshd, '-D', '-e', '-f', config] : ['-D','-e','-f',config], { windowsHide:true,stdio:['ignore','ignore','pipe'] });
}
async function stopDaemon(child, pidFile) {
  if (!child || child.exitCode !== null) return;
  const ended = new Promise(resolve=>child.once('exit',resolve));
  if (sudoTest) {
    const pid = fs.existsSync(pidFile) ? fs.readFileSync(pidFile, 'utf8').trim() : String(child.pid);
    if (!/^[1-9][0-9]*$/.test(pid)) throw new Error('Invalid fixture daemon pid');
    privileged(['kill','-TERM',pid]);
  }
  else child.kill();
  let timer;
  try {
    await Promise.race([ended,new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error('Fixture daemon did not stop')),5000);})]);
  } finally { clearTimeout(timer); }
}
try {
  const socket = net.createServer(); await new Promise(resolve => socket.listen(0, "127.0.0.1", resolve));
  const port = socket.address().port; await new Promise(resolve => socket.close(resolve));
  for (const name of ["host", "user"]) {
    const result = spawnSync("ssh-keygen", ["-q", "-t", "ed25519", "-N", "", "-f", path.join(directory, name)], { windowsHide: true, encoding: "utf8" });
    if (result.status !== 0) throw new Error(result.stderr || "ssh-keygen failed");
  }
  const username = os.userInfo().username;
  if (sudoTest) {
    privileged(['useradd','-M','-d','/tmp','-s','/bin/sh',sudoUser]);
    sudoAccount = true;
    privileged(['chpasswd'], `${sudoUser}:${sudoPassword}\n`);
    const policy=path.join(directory,'sudoers');
    fs.writeFileSync(policy,`Defaults:${sudoUser} timestamp_timeout=0,passwd_tries=3\n${sudoUser} ALL=(root) ALL\n`);
    privileged(['visudo','-c','-f',policy]);
    privileged(['install','-m','0440',policy,sudoers]);
    fs.chmodSync(directory,0o755);
    fs.chmodSync(path.join(directory,'user.pub'),0o644);
  }
  fs.writeFileSync(path.join(directory, "sshd_config"), [
    `Port ${port}`, "ListenAddress 127.0.0.1", `HostKey ${quote(path.join(directory, "host"))}`,
    `PidFile ${quote(path.join(directory, "pid"))}`, `AuthorizedKeysFile ${quote(path.join(directory, "user.pub"))}`,
    "StrictModes no", "PasswordAuthentication no", "KbdInteractiveAuthentication no", "PubkeyAuthentication yes",
    "AllowTcpForwarding yes", `Subsystem sftp ${process.env.SODECK_SFTP_SERVER ? quote(process.env.SODECK_SFTP_SERVER) : 'internal-sftp'}`, `LogLevel ${process.env.SODECK_SSHD_LOG_LEVEL ?? 'ERROR'}`, ...(process.platform === "win32" ? [] : [sudoTest ? "UsePAM yes" : "UsePAM no"]), "",
  ].join("\n"));
  let diagnostic = "";
  daemon = startDaemon(path.join(directory, "sshd_config"));
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
  let networkDeniedPort;
  if(flags.network){
    const available=net.createServer();await new Promise(r=>available.listen(0,'127.0.0.1',r));networkDeniedPort=available.address().port;await new Promise(r=>available.close(r));
    const config=path.join(directory,'sshd-network-denied.conf');fs.writeFileSync(config,fs.readFileSync(path.join(directory,'sshd_config'),'utf8').replace(`Port ${port}`,`Port ${networkDeniedPort}`).replace(`PidFile ${quote(path.join(directory,'pid'))}`,`PidFile ${quote(path.join(directory,'network-denied-pid'))}`).replace('AllowTcpForwarding yes','AllowTcpForwarding no'));
    const child=startDaemon(config);extraDaemons.push(child);child.stderr.on('data',data=>{diagnostic+=data;});let error;child.on('error',e=>{error=e;});
    let listening=false;for(let n=0;n<50;n++){await delay(100);if(error||child.exitCode!==null)throw new Error('Denied-forwarding SSH startup failed: '+(error??diagnostic));listening=await new Promise(r=>{const probe=net.connect({host:'127.0.0.1',port:networkDeniedPort});probe.on('connect',()=>{probe.destroy();r(true);});probe.on('error',()=>r(false));});if(listening)break;}if(!listening)throw new Error('Denied-forwarding SSH daemon did not listen');
  }
  const mcpPorts=[port];
  if(flags.mcp)for(let i=0;i<2;i++){
    const extraSocket=net.createServer();await new Promise(r=>extraSocket.listen(0,'127.0.0.1',r));const extraPort=extraSocket.address().port;await new Promise(r=>extraSocket.close(r));
    const config=path.join(directory,`sshd-mcp-${i}.conf`);fs.writeFileSync(config,fs.readFileSync(path.join(directory,'sshd_config'),'utf8').replace(`Port ${port}`,`Port ${extraPort}`).replace(`PidFile ${quote(path.join(directory,'pid'))}`,`PidFile ${quote(path.join(directory,'pid-'+i))}`));
    const child=startDaemon(config);extraDaemons.push(child);child.stderr.on('data',data=>{diagnostic+=data;});let error;child.on('error',e=>{error=e;});
    let listening=false;for(let n=0;n<50;n++){await delay(100);if(error||child.exitCode!==null)throw new Error('MCP SSH startup failed: '+(error??diagnostic));listening=await new Promise(r=>{const probe=net.connect({host:'127.0.0.1',port:extraPort});probe.on('connect',()=>{probe.destroy();r(true);});probe.on('error',()=>r(false));});if(listening)break;}if(!listening)throw new Error('MCP SSH daemon did not listen');mcpPorts.push(extraPort);
  }
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
    const env={ ...process.env, SODECK_TEST_SSH_PORT: String(port), SODECK_TEST_SSH_DENIED_PORT: String(networkDeniedPort??""), SODECK_TEST_SSH_USER: username, SODECK_TEST_SSH_KEY: path.join(directory, "user"),SODECK_TEST_MCP_PORTS:JSON.stringify(mcpPorts) };
    if(sudoTest){env.SODECK_TEST_SUDO_USER=sudoUser;env.SODECK_TEST_SUDO_PASSWORD=sudoPassword;env.SODECK_TEST_SUDO_MARKER=path.join(directory,'sudo-marker');}
    if(flags.mcp){env.SODECK_TEST_MCP_PLUGIN=testPluginDirectory('mcp');env.SODECK_TEST_MCP_OUTPUT=path.resolve('artifacts/mcp');}
    if(network) env.SODECK_TEST_CURL=await testCurl();
    for(const plugin of [terminal||flags.docker?'terminal':null,files?'files':null,network||flags.docker?'network-proxy':null,status?'system-status':null,flags.docker?'docker':null].filter(Boolean)){
      env[`SODECK_TEST_${(plugin==='network-proxy'?'NETWORK':plugin.toUpperCase().replaceAll('-','_'))}_PLUGIN`]=testPluginDirectory(plugin);
    }
    if(files){const root=env.SODECK_TEST_FILES_PLUGIN;const manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json'),'utf8'));const test=spawnSync(process.execPath,['scripts/test-files-process.mjs'],{stdio:'inherit',windowsHide:true,env:{...env,SODECK_TEST_FILES_EXECUTABLE:path.join(root,manifest.entry)}});if(test.status!==0)throw new Error('File process regressions failed');}
    if(network){const root=env.SODECK_TEST_NETWORK_PLUGIN;const manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json'),'utf8'));const test=spawnSync(process.execPath,['scripts/test-network-process.mjs'],{stdio:'inherit',windowsHide:true,env:{...env,SODECK_TEST_NETWORK_EXECUTABLE:path.join(root,manifest.entry)}});if(test.status!==0)throw new Error('Network process regressions failed');}
      const result = uiOnly ? 0 : await new Promise((resolve, reject) => {
    const test = spawn("cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml", "--features", "test-fixtures", "--locked", "--test", "connections", flags.docker?'openssh_docker_plugin':flags.mcp?'openssh_mcp_':status?'openssh_system_status_plugin':network?'network_proxy_plugin':files?'openssh_files_plugin':terminal?'openssh_terminal_plugin':'openssh_interop', "--", "--ignored", ...(flags.mcp?[]:["--exact"]), "--nocapture"], {
      windowsHide: true, stdio: "inherit", env,
    }); test.on("error", reject); test.on("exit", resolve);
  });
  if (result !== 0) throw new Error(`OpenSSH interoperability failed (${result}): ${diagnostic}`);
  if(network&&!uiOnly){const result=await new Promise((resolve,reject)=>{const test=spawn('cargo',['test','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--test','connections','openssh_network_plugin','--','--ignored','--exact','--nocapture'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('OpenSSH network plugin failed: '+diagnostic);}
  if(network&&!uiOnly){for(const name of ['network_socks_protocol','network_socks_lifecycle','network_forwarding_denied','openssh_network_forwarding_denied']){const result=await new Promise((resolve,reject)=>{const test=spawn('cargo',['test','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--test','connections',name,'--','--ignored','--exact','--nocapture'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error(name+' failed');}}
  if(network){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-network-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('Network UI failed');}
  if(status&&!communication){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-system-status-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('Status UI failed');}
  if(terminal&&!communication){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-terminal-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('Terminal UI failed');}
  if(files&&!communication){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-files-ui.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('File UI failed');}
  if(communication){const result=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-component-communication.mjs'],{stdio:'inherit',windowsHide:true,env});test.on('error',reject);test.on('exit',resolve);});if(result!==0)throw new Error('Component communication failed');}
  }
} finally {
  const stopped = await Promise.allSettled([
    ...extraDaemons.map((child, i) => stopDaemon(child, path.join(directory, 'pid-' + i))),
    stopDaemon(daemon, path.join(directory, 'pid')),
  ]);
  try {
    if (sudoAccount) {
      privileged(['rm', '-f', '--', sudoers]);
      // PAM may leave a session process after the test's SSH channels close.
      const terminated = spawnSync('sudo', ['-n', 'pkill', '-KILL', '-u', sudoUser], { encoding: 'utf8', windowsHide: true });
      if (![0, 1].includes(terminated.status)) throw new Error(terminated.stderr || 'Fixture session cleanup failed');
      const deadline = Date.now() + 5000;
      for (;;) {
        const removed = spawnSync('sudo', ['-n', 'userdel', sudoUser], { encoding: 'utf8', windowsHide: true });
        if (removed.status === 0) break;
        if (removed.status !== 8 || Date.now() >= deadline) throw new Error(removed.stderr || 'Fixture account cleanup failed');
        await delay(100);
      }
    }
  } finally {
    if (path.dirname(path.resolve(directory)) !== path.resolve(os.tmpdir()) || !path.basename(directory).startsWith("sodeck-openssh-")) throw new Error("Unexpected test cleanup path");
    fs.rmSync(directory, { recursive: true, force: true });
  }
  const failed = stopped.find(result => result.status === 'rejected');
  if (failed) throw failed.reason;
}
