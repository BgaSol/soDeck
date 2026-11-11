import * as monaco from 'monaco-editor/editor/editor.api.js';
import 'monaco-editor/languages/definitions/yaml/register.js';
import 'monaco-editor/languages/definitions/shell/register.js';
import 'monaco-editor/languages/definitions/powershell/register.js';
import 'monaco-editor/languages/definitions/ini/register.js';
import 'monaco-editor/languages/definitions/dockerfile/register.js';
import 'monaco-editor/languages/definitions/markdown/register.js';
import 'monaco-editor/languages/definitions/xml/register.js';
import 'monaco-editor/languages/definitions/html/register.js';
import 'monaco-editor/languages/definitions/css/register.js';
import 'monaco-editor/languages/definitions/javascript/register.js';
import 'monaco-editor/languages/definitions/typescript/register.js';
import 'monaco-editor/languages/definitions/python/register.js';
import 'monaco-editor/languages/definitions/sql/register.js';

export const languages: Record<string,string> = {plaintext:'纯文本',yaml:'YAML',json:'JSON',jsonc:'JSONC',shell:'Shell',powershell:'PowerShell',nginx:'Nginx',ini:'INI / systemd',toml:'TOML',dotenv:'环境变量',dockerfile:'Dockerfile',markdown:'Markdown',xml:'XML',html:'HTML',css:'CSS',javascript:'JavaScript',typescript:'TypeScript',python:'Python',sql:'SQL'};
for(const id of ['json','jsonc','nginx','toml','dotenv'])monaco.languages.register({id});
const strings: monaco.languages.IMonarchLanguage['tokenizer'] = {
  double: [[/[^\\"$]+/,'string'],[/\\./,'string.escape'],[/\$(?:\{[^}]*\}|\w+)/,'variable'],[/"/,'string','@pop']],
  single: [[/[^']+/,'string'],[/'/,'string','@pop']],
};
for(const id of ['json','jsonc'])monaco.languages.setMonarchTokensProvider(id,{tokenizer:{root:[...(id==='jsonc'?[[/\/\/.*$/,'comment'],[/\/\*/,'comment','@comment']]:[]) as monaco.languages.IMonarchLanguageRule[],[/"(?:[^"\\]|\\.)*"(?=\s*:)/,'key'],[/"/,'string','@double'],[/\b(?:true|false|null)\b/,'keyword'],[/-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/,'number'],[/[{}[\],:]/,'delimiter']],double:[[/[^\\"]+/,'string'],[/\\./,'string.escape'],[/"/,'string','@pop']],comment:[[/[^*/]+/,'comment'],[/\*\//,'comment','@pop'],[/[*/]/,'comment']]}});
monaco.languages.setMonarchTokensProvider('nginx',{tokenizer:{root:[[/#.*$/,'comment'],[/\$[\w]+/,'variable'],[/"/,'string','@double'],[/'/,'string','@single'],[/^\s*[\w]+/,'keyword'],[/\b\d+(?:[kKmMgG]|ms|s)?\b/,'number'],[/[{};]/,'delimiter']],...strings}});
monaco.languages.setMonarchTokensProvider('dotenv',{tokenizer:{root:[[/#.*$/,'comment'],[/^\s*(?:export\s+)?[\w]+(?=\s*=)/,'key'],[/\$\{[^}]+\}|\$\w+/,'variable'],[/"/,'string','@double'],[/'/,'string','@single'],[/=/,'delimiter']],...strings}});
monaco.languages.setMonarchTokensProvider('toml',{tokenizer:{root:[[/#.*$/,'comment'],[/\[\[?[^\]]+\]\]?/,'type'],[/[\w.-]+(?=\s*=)/,'key'],[/"""/,'string','@multidouble'],[/'''/,'string','@multisingle'],[/"/,'string','@double'],[/'/,'string','@single'],[/\b(?:true|false)\b/,'keyword'],[/\b\d{4}-\d\d-\d\d(?:[Tt ][\d:.+Zz-]+)?/,'number'],[/[+-]?(?:\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?|inf|nan)/,'number'],[/[{}[\],=]/,'delimiter']],...strings,multidouble:[[/"""/,'string','@pop'],[/\\./,'string.escape'],[/./,'string']],multisingle:[[/'''/,'string','@pop'],[/./,'string']]}});

export function detectLanguage(path:string,text:string):string {
  const name=path.split('/').pop()||'',lower=name.toLowerCase();
  if(/^(Dockerfile|Containerfile)(\.|$)/.test(name))return 'dockerfile';
  if(lower==='.env'||lower.startsWith('.env.')||lower.endsWith('.env'))return 'dotenv';
  if(name==='nginx.conf')return 'nginx';
  if(['.bashrc','.bash_profile','.profile','.zshrc'].includes(name))return 'shell';
  const ext=lower.split('.').pop()||'';
  const map:Record<string,string>={yaml:'yaml',yml:'yaml',json:'json',jsonc:'jsonc',sh:'shell',bash:'shell',zsh:'shell',ps1:'powershell',psm1:'powershell',psd1:'powershell',ini:'ini',service:'ini',socket:'ini',timer:'ini',target:'ini',mount:'ini',automount:'ini',path:'ini',toml:'toml',md:'markdown',xml:'xml',html:'html',htm:'html',css:'css',js:'javascript',mjs:'javascript',cjs:'javascript',ts:'typescript',py:'python',sql:'sql'};
  if(map[ext])return map[ext];
  const first=text.slice(0,512).split('\n')[0];
  if(/^#!.*(?:\/|\s)(?:ba|z|da|k)?sh(?:\s|$)/.test(first))return 'shell';
  if(/^#!.*(?:\/|\s)pwsh(?:\s|$)/.test(first))return 'powershell';
  if(/^#!.*(?:\/|\s)python[\d.]*(?:\s|$)/.test(first))return 'python';
  if(ext==='conf'&&path.startsWith('/etc/nginx/'))return 'nginx';
  return 'plaintext';
}
