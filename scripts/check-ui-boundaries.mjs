import ts from 'typescript';
import postcss from 'postcss';
import selectorParser from 'postcss-selector-parser';
import {readFile,readdir} from 'node:fs/promises';
import {resolve,relative,posix} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=fileURLToPath(new URL('../',import.meta.url));
const layout=new Set('display position inset top right bottom left width min-width max-width height min-height max-height flex flex-direction flex-wrap flex-shrink flex-grow flex-basis align-items align-self justify-content gap row-gap column-gap padding padding-inline padding-block padding-top padding-right padding-bottom padding-left margin margin-top margin-bottom margin-left margin-right overflow overflow-x overflow-y overflow-wrap white-space text-overflow text-align line-height visibility pointer-events user-select z-index grid-template-columns grid-template-rows touch-action'.split(' '));
// Narrow vendor/host-surface exceptions. No generic button/list skins are exempted.
const adapterSelectors=new Map([
 ['plugins/terminal/src/terminal.css',new Set([':root','.terminal-view','.xterm .xterm-viewport','.xterm .xterm-scrollable-element>.visible','.xterm .composition-view','.xterm .xterm-accessibility-debug:focus'])],
 ['plugins/files/src/files.css',new Set([':root','.file-tree:focus-visible'])],
]);
const geometry=new Set(['height','width','left','top','paddingLeft']);
export function styleErrors(file,source){
 file=file.replaceAll('\\','/');const errors=[];
 if(file.endsWith('.css')){let ast;try{ast=postcss.parse(source);}catch(e){return [e.message];}
 ast.walkAtRules('import',rule=>{const target=/["']([^"']+)["']/.exec(rule.params)?.[1];if(target){const resolved=target.startsWith('.')?posix.normalize(posix.join(posix.dirname(file),target)):target;if(/^(apps\/|crates\/|packages\/|plugins\/)/.test(resolved)&&!resolved.startsWith(`plugins/${file.split('/')[1]}/`))errors.push('Cross-package private stylesheet is forbidden');}});
 ast.walkRules(rule=>{let theme=false;selectorParser(sel=>{sel.walkAttributes(a=>{if(/data-(theme|high-contrast)/.test(a.attribute))theme=true;});}).processSync(rule.selector);if(theme)errors.push('Plugin must not branch on theme');
 rule.walkDecls(d=>{if(d.prop.startsWith("--sd-"))errors.push("Plugin must not override public tokens");if(/#[\da-f]{3,8}\b|\b(?:rgb|hsl)a?\(/i.test(d.value))errors.push(`Fixed color in ${rule.selector}`);if(!layout.has(d.prop)&&!adapterSelectors.get(file)?.has(rule.selector))errors.push(`Visual ${d.prop} in ${rule.selector} belongs in public UI`);});});
 }else{const ast=ts.createSourceFile(file,source,ts.ScriptTarget.Latest,true,file.endsWith('tsx')?ts.ScriptKind.TSX:ts.ScriptKind.TS);
 const visit=node=>{
  if(ts.isJsxAttribute(node)&&node.name.getText(ast)==='style'){
   const expression=node.initializer&&ts.isJsxExpression(node.initializer)?node.initializer.expression:undefined;
   if(!expression||!ts.isObjectLiteralExpression(expression))errors.push('Inline styles must be explicit geometry objects');
   else for(const prop of expression.properties)if(!ts.isPropertyAssignment(prop)||!geometry.has(prop.name.getText(ast)))errors.push('Inline visual style belongs in public UI');
  }
  if(ts.isJsxOpeningElement(node)||ts.isJsxSelfClosingElement(node)){if(['style','button','input','select'].includes(node.tagName.getText(ast)))errors.push(`Use public ${node.tagName.getText(ast)} control`);}
  if(ts.isPropertyAccessExpression(node)&&node.expression.getText(ast).endsWith('dataset')&&['theme','highContrast'].includes(node.name.text))errors.push('Plugin must not branch on theme');
  if(ts.isBinaryExpression(node)&&[ts.SyntaxKind.EqualsEqualsEqualsToken,ts.SyntaxKind.ExclamationEqualsEqualsToken,ts.SyntaxKind.EqualsEqualsToken,ts.SyntaxKind.ExclamationEqualsToken].includes(node.operatorToken.kind)&&[node.left,node.right].some(v=>ts.isStringLiteral(v)&&['dopamine','soft-light','soft-dark'].includes(v.text)))errors.push('Plugin must not branch on built-in theme identity');
  if(ts.isCallExpression(node)&&/createElement/.test(node.expression.getText(ast))&&node.arguments[0]&&ts.isStringLiteral(node.arguments[0])&&node.arguments[0].text==='style')errors.push('Runtime stylesheet injection is forbidden');
  ts.forEachChild(node,visit);
 };visit(ast);
 }
 return [...new Set(errors)];
}
async function walk(dir){const out=[];for(const entry of await readdir(dir,{withFileTypes:true})){if(['dist','node_modules'].includes(entry.name))continue;const path=resolve(dir,entry.name);if(entry.isDirectory())out.push(...await walk(path));else if(/\.(tsx?|css)$/.test(entry.name))out.push(path);}return out;}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const errors=[];for(const plugin of (await readdir(resolve(root,'plugins'),{withFileTypes:true})).filter(p=>p.isDirectory()).map(p=>p.name))for(const file of await walk(resolve(root,`plugins/${plugin}/src`))){if(file.endsWith('.test.ts'))continue;for(const error of styleErrors(relative(root,file),await readFile(file,'utf8')))errors.push(`${relative(root,file)}: ${error}`);}
 if(errors.length)throw new Error(errors.join('\n'));console.log('Plugin UI boundaries passed (AST + PostCSS).');
}
