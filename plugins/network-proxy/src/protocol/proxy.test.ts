import {describe,it,expect,vi} from 'vitest';
import {proxy} from './index';
import type {Channel,Connect} from './channel';
import {ascii} from './io';
import {fault} from './address';

class Memory implements Channel {
  queue:Uint8Array[]=[];output:Uint8Array[]=[];ended=false;closed=false;
  spliceTail?:[string,string];waiters=new Set<()=>void>();
  onWrite?:(text:string)=>void;
  constructor(text?:string){if(text!==undefined){this.feed(text);this.end();}}
  get eof(){return this.ended&&!this.queue.length;}
  feed(value:string|Uint8Array){const bytes=typeof value==='string'?ascii(value):value;if(bytes.length)this.queue.push(bytes);this.wake();}
  end(){this.ended=true;this.wake();}
  wake(){for(const fn of [...this.waiters])fn();}
  peek(max=65536){return this.queue[0]?.subarray(0,max)??new Uint8Array();}
  async readable(deadline?:number):Promise<Uint8Array> {
    while(!this.queue.length&&!this.ended){await new Promise<void>((resolve,reject)=>{
      const timer=deadline===undefined?undefined:setTimeout(()=>{this.waiters.delete(done);reject(fault('TIMEOUT','timeout'));},Math.max(0,deadline-Date.now()));
      const done=()=>{clearTimeout(timer);this.waiters.delete(done);resolve();};this.waiters.add(done);
    });}
    return this.peek();
  }
  consume(n:number){while(n){const b=this.queue[0];if(n<b.length){this.queue[0]=b.subarray(n);return;}n-=b.length;this.queue.shift();}}
  async write(bytes:Uint8Array){if(this.closed)throw fault('CANCELED','closed');this.output.push(bytes.slice());this.onWrite?.(String.fromCharCode(...bytes));}
  get text(){return this.output.map(b=>String.fromCharCode(...b)).join('');}
  get tail(){return this.queue.map(b=>String.fromCharCode(...b)).join('');}
  async close(){this.closed=true;this.end();}
  async splice(other:Channel){this.spliceTail=[this.tail,(other as Memory).tail];}
}
const get=(host:string,extra='')=>`GET http://${host}/page HTTP/1.1\r\nHost: ${host}\r\n${extra}\r\n`;
describe('HTTP proxy wire behavior',()=>{
  it.each(['Broken header','Content-Length: 3\r\nTransfer-Encoding: chunked','Content-Length: nope','Connection: Host'])('reports malformed upstream fields as a gateway failure: %s',async header=>{
    const client=new Memory(get('x')),remote=new Memory(`HTTP/1.1 200 OK\r\n${header}\r\n\r\n`);
    await expect(proxy(client,async()=>remote)).rejects.toMatchObject({code:'BAD_GATEWAY'});expect(client.text).toContain('502 Bad Gateway');expect(client.text).not.toContain('400');
  });
  it('rewrites absolute targets, removes proxy headers, and keeps a pipelined client across targets',async()=>{
    const client=new Memory(get('one.internal','Proxy-Authorization: secret\r\n')+get('two.internal','Connection: close\r\n'));
    const peers:Memory[]=[],targets:string[]=[];
    await proxy(client,async target=>{targets.push(target.host);const peer=new Memory('HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc');peers.push(peer);return peer;});
    expect(targets).toEqual(['one.internal','two.internal']);
    expect(peers[0].text).toContain('GET /page HTTP/1.1\r\n');expect(peers[0].text).not.toContain('secret');
    expect(client.text.match(/abc/g)).toHaveLength(2);expect(peers.every(p=>p.closed)).toBe(true);
  });
  it.each([
    get('one.internal').replace('Host: one.internal','Host: two.internal'),
    get('one.internal','Content-Length: 4\r\nTransfer-Encoding: chunked\r\n'),
    get('one.internal','Content-Length: 4\r\nContent-Length: 4\r\n'),
    get('one.internal','Connection: Content-Length\r\nContent-Length: 4\r\n'),
    'GET / HTTP/1.1\nHost: x\n\n',
  ])('rejects ambiguous requests before opening any target',async request=>{
    const client=new Memory(request),connect=vi.fn<Connect>();await expect(proxy(client,connect)).rejects.toBeTruthy();
    expect(connect).not.toHaveBeenCalled();expect(client.text).toContain('400 Bad Request');
  });
  it('enforces header and start line limits',async()=>{
    for(const request of [get('x',Array.from({length:101},()=> 'X: v\r\n').join('')),get('x','X: '+ 'a'.repeat(65536)+'\r\n'),'GET /'+ 'a'.repeat(8192)+' HTTP/1.1\r\n\r\n']){
      const client=new Memory(request);await expect(proxy(client,vi.fn())).rejects.toBeTruthy();expect(client.text).toContain('431');
    }
  });
  it('waits for CONNECT target and preserves handshake tail on both sides',async()=>{
    const client=new Memory('CONNECT example.internal:443 HTTP/1.1\r\nHost: example.internal:443\r\n\r\nTLS'),peer=new Memory('SERVER');
    let release!:(peer:Memory)=>void;const running=proxy(client,()=>new Promise(r=>{release=r;}));
    await vi.waitFor(()=>expect(release).toBeTypeOf('function'));expect(client.text).toBe('');release(peer);
    expect(await running).toBe(true);expect(client.text).toContain('200 Connection Established');expect(client.spliceTail).toEqual(['TLS','SERVER']);
  });
  it('relays 100 Continue without waiting for the request body and streams chunked trailers',async()=>{
    const client=new Memory(),peer=new Memory();client.feed('POST http://x/upload HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n');
    peer.onWrite=()=>{
      if(peer.text.endsWith('connection: close\r\n\r\n'))peer.feed('HTTP/1.1 100 Continue\r\n\r\n');
      if(peer.text.endsWith('0\r\nx-check: yes\r\n\r\n'))peer.feed('HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\nX-End: yes\r\n\r\n');
    };
    client.onWrite=text=>{if(text.includes('100 Continue')){client.feed('3;extension=yes\r\nabc\r\n0\r\nX-Check: yes\r\n\r\n');client.end();}};
    await proxy(client,async()=>peer);expect(peer.text).toContain('3\r\nabc\r\n0\r\nx-check: yes');expect(client.text).toContain('2\r\nok\r\n0\r\nx-end: yes');
  });
  it('delivers an early final response even when the client is waiting to upload',async()=>{
    const client=new Memory(),peer=new Memory('HTTP/1.1 413 Content Too Large\r\nContent-Length: 2\r\n\r\nno');
    client.feed('POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 1000\r\nExpect: 100-continue\r\n\r\n');
    expect(await proxy(client,async()=>peer)).toBe(false);expect(client.text).toContain('connection: close\r\n');expect(client.text.endsWith('no')).toBe(true);await client.close();
  });
  it('preserves response boundaries for HEAD, 204 and 304',async()=>{
    const client=new Memory('HEAD / HTTP/1.1\r\nHost: x\r\n\r\n'+get('x')+get('x','Connection: close\r\n'));
    const responses=['200 OK','204 No Content','304 Not Modified'];
    await proxy(client,async()=>new Memory(`HTTP/1.1 ${responses.shift()}\r\nContent-Length: 123\r\n\r\n`));
    expect(client.text.match(/HTTP\/1.1/g)).toHaveLength(3);
  });
  it('streams close-delimited SSE and closes the downstream after EOF',async()=>{
    const client=new Memory(get('x')),peer=new Memory();peer.feed('HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: 1\n\n');
    const running=proxy(client,async()=>peer);await vi.waitFor(()=>expect(client.text).toContain('data: 1'));
    peer.feed('data: 2\n\n');peer.end();expect(await running).toBe(false);expect(client.text).toContain('data: 2');
  });
  it('splices WebSocket only after a matching 101 and preserves frame tails',async()=>{
    const client=new Memory(get('x','Connection: Upgrade\r\nUpgrade: websocket\r\n')+'FRAME'),peer=new Memory('HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\nREPLY');
    expect(await proxy(client,async()=>peer)).toBe(true);expect(client.spliceTail).toEqual(['FRAME','REPLY']);
  });
  it('maps target failure to a proxy error and never reports CONNECT success',async()=>{
    const client=new Memory('CONNECT x:443 HTTP/1.1\r\nHost: x:443\r\n\r\n');
    await expect(proxy(client,async()=>{throw fault('TARGET_REFUSED','refused');})).rejects.toBeTruthy();expect(client.text).toContain('502');expect(client.text).not.toContain('200');
  });
});
describe('SOCKS5',()=>{
  it.each([1,3,4])('opens address type %i and preserves post-handshake data',async type=>{
    const client=new Memory();client.feed(Uint8Array.of(5,1,0,5,1,0,type));
    client.feed(type===1?Uint8Array.of(192,0,2,1):type===3?Uint8Array.of(10,...ascii('x.internal')):Uint8Array.of(0x20,1,0xd,0xb8,...new Uint8Array(12)));
    client.feed(Uint8Array.of(1,187,...ascii('TAIL')));client.end();
    const connect=vi.fn<Connect>(async()=>new Memory('REMOTE'));
    expect(await proxy(client,connect)).toBe(true);expect(connect.mock.calls[0][0].port).toBe(443);expect(client.output[1][1]).toBe(0);expect(client.spliceTail).toEqual(['TAIL','REMOTE']);
    if(type===3)expect(connect.mock.calls[0][0].host).toBe('x.internal');
  });
  it.each([2,3])('rejects unsupported command %i',async command=>{
    const client=new Memory();client.feed(Uint8Array.of(5,1,0,5,command,0,1));client.end();const connect=vi.fn<Connect>();
    expect(await proxy(client,connect)).toBe(false);expect(client.output[1][1]).toBe(7);expect(connect).not.toHaveBeenCalled();
  });
  it('rejects password-only method negotiation',async()=>{
    const client=new Memory();client.feed(Uint8Array.of(5,1,2));client.end();expect(await proxy(client,vi.fn())).toBe(false);expect(client.output[0]).toEqual(Uint8Array.of(5,255));
  });
});
