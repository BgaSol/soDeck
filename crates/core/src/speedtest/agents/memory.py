"""SoDeck memory speed test v1. Fixed exec template; no files or listeners."""
import hashlib
import json
import os
import struct
import sys
import threading
import time

HEADER = struct.Struct('!4sBBBBII')
HELLO, CONFIG, READY, BEGIN, DATA, ACK, END, RESULT, CANCEL = range(1, 10)
inp, out = sys.stdin.buffer, sys.stdout.buffer
tx = rx = control = 0
stop = threading.Event()

def watchdog():
    if not stop.wait(60):
        os._exit(124)

threading.Thread(target=watchdog, daemon=True).start()

def exact(size):
    result = bytearray()
    while len(result) < size:
        part = inp.read(size - len(result))
        if not part:
            raise EOFError()
        result.extend(part)
    return bytes(result)

def frame(kind, phase, payload):
    global tx, control
    if kind != DATA:
        payload = json.dumps(payload, separators=(',', ':')).encode('utf-8')
        control += len(payload)
        if len(payload) > 4096 or control > 65536:
            raise ValueError('control budget')
    if len(payload) > 65536 or tx >= 0xffffffff:
        raise ValueError('frame budget')
    out.write(HEADER.pack(b'SDS1', 1, kind, phase, 0, tx, len(payload)))
    out.write(payload)
    out.flush()
    tx += 1

def read_frame():
    global rx, control
    magic, version, kind, phase, flags, sequence, size = HEADER.unpack(exact(16))
    if magic != b'SDS1' or version != 1 or flags or sequence != rx:
        raise ValueError('header')
    if size > 65536 or kind not in range(1, 10) or (kind != DATA and size > 4096):
        raise ValueError('length/type')
    rx += 1
    payload = exact(size)
    if kind != DATA:
        control += size
        if control > 65536:
            raise ValueError('control budget')
        payload = json.loads(payload)
    if kind == CANCEL:
        raise EOFError()
    return kind, phase, payload

def run():
    frame(HELLO, 0, {'method': 'python-memory-v1', 'protocol': 1, 'hardDeadlineSeconds': 60})
    kind, phase, config = read_frame()
    if kind != CONFIG or phase != 0 or set(config) != {'profile', 'directions'}:
        raise ValueError('configuration')
    if config['profile'] not in ('standard', 'save-data'):
        raise ValueError('profile')
    directions = config['directions']
    if directions not in (['upload'], ['download'], ['upload', 'download']):
        raise ValueError('directions')
    limit, warm, formal = (256 * 1048576, 2, 10) if config['profile'] == 'standard' else (32 * 1048576, 1, 5)
    frame(READY, 0, {'ready': True})
    for direction in directions:
        remaining = limit
        for formal_phase in (False, True):
            expected = (1 if direction == 'upload' else 3) + int(formal_phase)
            seconds = formal if formal_phase else warm
            kind, phase, begin = read_frame()
            if kind != BEGIN or phase != expected or begin != {}:
                raise ValueError('phase')
            digest = hashlib.sha256()
            count = 0
            started = last_ack = time.monotonic()
            # Bound warmup to half the byte allowance so that a fast link still
            # has a formal interval. The total across both phases remains fixed.
            phase_limit = remaining if formal_phase else limit // 2
            if direction == 'upload':
                while True:
                    kind, got_phase, payload = read_frame()
                    if got_phase != phase:
                        raise ValueError('phase mismatch')
                    if kind == END:
                        if set(payload) != {'bytes', 'sha256'} or payload['bytes'] != str(count) or payload['sha256'] != digest.hexdigest():
                            raise ValueError('verification')
                        break
                    if kind != DATA or not payload or count + len(payload) > phase_limit:
                        raise ValueError('data budget')
                    count += len(payload)
                    digest.update(payload)
                    now = time.monotonic()
                    if now - last_ack >= .25:
                        frame(ACK, phase, {'bytes': str(count)})
                        last_ack = now
            else:
                while count < phase_limit and time.monotonic() - started < seconds:
                    payload = os.urandom(min(16384, phase_limit - count))
                    frame(DATA, phase, payload)
                    digest.update(payload)
                    count += len(payload)
            frame(RESULT, phase, {'bytes': str(count), 'sha256': digest.hexdigest(), 'verified': True})
            if direction == 'download':
                kind, got_phase, receipt = read_frame()
                if kind != ACK or got_phase != phase or receipt != {'bytes': str(count), 'sha256': digest.hexdigest()}:
                    raise ValueError('receipt')
            remaining -= count
    frame(RESULT, 0, {'complete': True})

try:
    run()
except EOFError:
    pass
except Exception:
    sys.stderr.write('SODECK_SPEEDTEST_PROTOCOL_ERROR\n')
    sys.exit(2)
finally:
    stop.set()
