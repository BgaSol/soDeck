import {it,expect} from 'vitest';
import {diagnosticReference,diagnosticCause,sameDiagnostic,validateConnectionDiagnostics} from './connection-diagnostics';
it('accepts only exact safe diagnostic references and checks host open payloads',()=>{
 const ref={bootId:'boot',attemptId:'attempt',attemptGeneration:2};
 expect(diagnosticReference({details:{diagnosticRef:ref}})).toEqual(ref);
 expect(diagnosticReference({details:{diagnosticRef:{...ref,password:'private'}}})).toBeUndefined();
 expect(sameDiagnostic(ref,{...ref,bootId:'old-boot'})).toBe(false);
 expect(sameDiagnostic(ref,{...ref,attemptGeneration:3})).toBe(false);
 expect(validateConnectionDiagnostics('x-methods','ui.connectionDiagnostics.open',{reference:ref,workspaceId:'spoof'})).toBeTruthy();
});
it('separates observed EOF from unsupported root-cause claims and labels proxy replies',()=>{
 const text=diagnosticCause({code:'PROXY_DISCONNECTED',stage:'proxyNegotiate',receivedBytes:0,expectedBytes:2});
 expect(text).toContain('通道关闭');expect(text).not.toMatch(/未监听|密码/);
 expect(diagnosticCause({code:'PROXY_CONNECT_FAILED',reply:5})).toBe('代理返回：连接被拒绝（REP 5）');
});
