/** Connection challenges are handled by the host, including preset consent. */
export function isConnectionAuthenticationError(error:unknown):boolean {
  const code=typeof error==='string'?error:error&&typeof error==='object'&&'code' in error?error.code:undefined;
  return typeof code==='string'&&['HOST_KEY_REQUIRED','HOST_KEY_CHANGED','AUTH_REQUIRED','AUTH_FAILED','PROXY_AUTH_REQUIRED','PROXY_AUTH_FAILED','CREDENTIAL_CONFIRM_REQUIRED'].includes(code);
}
