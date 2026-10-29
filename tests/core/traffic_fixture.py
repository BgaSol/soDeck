"""Independent E3 wire client, restricted to the dedicated loopback fixture.

Protocol probes only: this is not evidence of native desktop behavior.
"""
import argparse
import base64
import hashlib
import gzip
import json
import os
from pathlib import Path
import struct
import time
import paramiko

parser = argparse.ArgumentParser()
parser.add_argument('--trace-agent', action='store_true')
parser.add_argument('--bytes', type=int, default=65536)
parser.add_argument('--frames', type=int, default=1)
parser.add_argument('--eof', action='store_true')
parser.add_argument('--powershell7', action='store_true')
parser.add_argument('--pulse', action='store_true')
parser.add_argument('--async-read', action='store_true')
parser.add_argument('--inline-script', action='store_true')
parser.add_argument('--raw-echo', action='store_true')
parser.add_argument('--command-flag', action='store_true')
parser.add_argument('--file-stream', action='store_true')
parser.add_argument('--overlapped', action='store_true')
parser.add_argument('--raw-chunk', type=int, default=0)
parser.add_argument('--pipe-stream', action='store_true')
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
node = json.loads((root / '.qa/t019/config.json').read_text(encoding='utf-8-sig'))['nodes'][0]
assert node['host'] == '127.0.0.1' and 22329 <= node['port'] <= 22339
source = (root / 'crates/core/src/speedtest/agents/memory.ps1').read_text(encoding='utf-8')
if args.pulse:
    source = source.replace('$offset += $count', "$offset += $count; if ($length -gt 4096) { Write-Frame 6 1 @{bytes='0'} }")
if args.async_read:
    source = source.replace('$count = $script:InputStream.Read($buffer, $offset, [Math]::Min(16384, $length - $offset))', '$pendingRead = $script:InputStream.ReadAsync($buffer, $offset, [Math]::Min(16384, $length - $offset)); while (-not $pendingRead.Wait(250)) { if ($script:ReadingData) { Write-Frame 6 1 @{bytes="0"} } }; $count=$pendingRead.Result')
if args.trace_agent:
    source = source.replace('$offset += $count', "$offset += $count; [Console]::Error.WriteLine(('read-part:{0}:{1}:{2}' -f $length,$offset,$count))")
    source = source.replace('return ,$buffer', "[Console]::Error.WriteLine(('read-exact:{0}' -f $length)); return ,$buffer")
    source = source.replace('return [pscustomobject]@{Kind=', "[Console]::Error.WriteLine(('frame:{0}:{1}:{2}' -f $kind,$phase,$length)); return [pscustomobject]@{Kind=")
    source = source.replace("$count += $payload.Length", "$count += $payload.Length; [Console]::Error.WriteLine(('hashed:{0}' -f $count))")
source = source.encode('utf-8')
loader = ('$script:BootstrapBytes=' + str(len(source)) + ';$s=[Console]::OpenStandardInput();'
          '$b=[byte[]]::new($script:BootstrapBytes);$o=0;while($o -lt $b.Length)'
          '{$n=$s.Read($b,$o,$b.Length-$o);if($n -eq 0){exit 2};$o+=$n};'
          '$r=[PowerShell]::Create();[void]$r.AddScript(([string]::Concat(\'$script:BootstrapBytes=\',$script:BootstrapBytes,\';\',[Text.Encoding]::UTF8.GetString($b))));$r.Invoke();$r.Dispose()')
if args.inline_script:
    packed = base64.b64encode(gzip.compress(source)).decode()
    loader = ("$script:BootstrapBytes=0;$m=[IO.MemoryStream]::new([Convert]::FromBase64String('" + packed + "'));"
              "$g=[IO.Compression.GZipStream]::new($m,[IO.Compression.CompressionMode]::Decompress);"
              "$t=[IO.StreamReader]::new($g);& ([scriptblock]::Create($t.ReadToEnd()))")
if args.raw_echo:
    loader = '[Console]::OpenStandardInput().CopyTo([Console]::OpenStandardOutput())'
    if args.raw_chunk:
        loader = ('$i=[Console]::OpenStandardInput();$o=[Console]::OpenStandardOutput();$b=[byte[]]::new(' + str(args.raw_chunk) + ');while(($n=$i.Read($b,0,$b.Length)) -gt 0){$o.Write($b,0,$n);$o.Flush()}')
if args.file_stream:
    replacement = "[IO.FileStream]::new([Microsoft.Win32.SafeHandles.SafeFileHandle]::new([IntPtr]::new(3),$false),[IO.FileAccess]::Read)"
    # Get the actual inherited handle without P/Invoke or assuming its number.
    replacement = "[IO.FileStream]::new([Microsoft.Win32.SafeHandles.SafeFileHandle]::new(([Console]::OpenStandardInput().GetType().GetField('_handle',[Reflection.BindingFlags]'Instance,NonPublic').GetValue([Console]::OpenStandardInput())).DangerousGetHandle(),$false),[IO.FileAccess]::Read)"
    loader = loader.replace('[Console]::OpenStandardInput()', replacement)
    if args.overlapped:
        loader = loader.replace('[IO.FileAccess]::Read)', '[IO.FileAccess]::Read,16384,$true)')
if args.pipe_stream:
    replacement = "[IO.Pipes.AnonymousPipeClientStream]::new([IO.Pipes.PipeDirection]::In,[Microsoft.Win32.SafeHandles.SafePipeHandle]::new(([Console]::OpenStandardInput().GetType().GetField('_handle',[Reflection.BindingFlags]'Instance,NonPublic').GetValue([Console]::OpenStandardInput())).DangerousGetHandle(),$false))"
    loader = loader.replace('[Console]::OpenStandardInput()', replacement)
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect(node['host'], port=node['port'], username=node['username'], key_filename=node['key'])
channel = client.get_transport().open_session()
channel.settimeout(5)
executable = ('"C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/native/powershell/pwsh.exe"' if args.powershell7 else 'powershell.exe')
command = executable + ' -NoProfile -NonInteractive -EncodedCommand ' + base64.b64encode(loader.encode('utf-16le')).decode()
if args.command_flag:
    command = executable + ' -NoProfile -NonInteractive -Command "& ([scriptblock]::Create([Text.Encoding]::Unicode.GetString([Convert]::FromBase64String(\'' + base64.b64encode(loader.encode('utf-16le')).decode() + '\'))))"'
assert len(command) < 8191, len(command)
channel.exec_command(command)
if not args.inline_script and not args.raw_echo:
    channel.sendall(source)

def exact(size):
    data = b''
    while len(data) < size:
        try:
            chunk = channel.recv(size - len(data))
        except TimeoutError:
            print('read timeout at', len(data), 'of', size, flush=True)
            if args.raw_echo:
                print('received prefix matches', data == payload[:len(data)], flush=True)
            raise
        if not chunk:
            raise EOFError()
        data += chunk
    return data

def read():
    header = struct.unpack('!4sBBBBII', exact(16))
    assert header[0] == b'SDS1' and header[-1] <= 65536
    payload = exact(header[-1])
    print('received', header[2:6], payload[:160], flush=True)
    return header, payload

def send(kind, phase, seq, data):
    payload = data if kind == 5 else json.dumps(data, separators=(',', ':')).encode()
    channel.sendall(struct.pack('!4sBBBBII', b'SDS1', 1, kind, phase, 0, seq, len(payload)))
    channel.sendall(payload)

try:
    if args.raw_echo:
        import threading
        payload = os.urandom(args.bytes * args.frames)
        sender = threading.Thread(target=lambda: (channel.sendall(payload), channel.shutdown_write()))
        sender.start()
        response = exact(len(payload))
        assert hashlib.sha256(response).digest() == hashlib.sha256(payload).digest()
        sender.join(5)
        assert not sender.is_alive()
        print('raw binary echo verified', len(payload), flush=True)
        raise SystemExit(0)
    read()
    send(2, 0, 0, {'profile': 'save-data', 'directions': ['upload']})
    read()
    send(4, 1, 1, {})
    payload = os.urandom(args.bytes)
    digest = hashlib.sha256()
    for index in range(args.frames):
        send(5, 1, 2 + index, payload)
        digest.update(payload)
    send(7, 1, 2 + args.frames, {'bytes': str(len(payload)*args.frames), 'sha256': digest.hexdigest()})
    if args.eof:
        channel.shutdown_write()
    while read()[0][2] != 8:
        pass
finally:
    if channel.recv_stderr_ready():
        print('fixture stderr:', channel.recv_stderr(8192).decode(errors='replace'), flush=True)
    channel.shutdown_write()
    channel.close()
    client.close()
