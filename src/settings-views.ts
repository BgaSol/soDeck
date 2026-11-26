import { themes } from './theme';
import type { Snapshot } from './core';
import type { GeneralSettingsState } from './general-settings';
import { button as b, escape as e, icon } from './ui';

const builtinSections = [
  { id: 'general', title: '通用', icon: 'settings' },
  { id: 'about', title: '应用与数据', icon: 'info' },
] as const;
type BuiltinSection = typeof builtinSections[number]['id'];
export type SettingsSection = BuiltinSection | 'components';
export function isBuiltinSettingsSection(value: string): value is BuiltinSection {
  return builtinSections.some(section => section.id === value);
}
export function settingsNavigation(section: SettingsSection | null, componentNavigation: string) {
  return builtinSections.map(({ id, title, icon: symbol }, index) =>
    `${index ? componentNavigation : ''}<li>${b('section', `<span>${title}</span>`, symbol, 'console-settings-item', `id="settings-nav-${id}" data-id="${id}" aria-current="${id === section ? 'page' : 'false'}"`)}</li>`,
  ).join('');
}
export function settings(s: Snapshot, section: SettingsSection, state?: GeneralSettingsState) {
  return `<section class="settings-page" data-scroll="settings" data-settings-section="${section}" ${section === 'components' ? 'id="component-settings-page"' : ''}>${section === 'components' ? '' : section === 'general' ? general(s, state!) : about(s)}</section>`;
}
function feedback(id: string, text: string, error = false) {
  return `<div id="${id}" role="status" aria-live="polite" class="hint settings-field-feedback ${error ? 'error-text' : ''}">${e(text)}</div>`;
}
function general(s: Snapshot, { theme, close, startup }: GeneralSettingsState) {
  const selectedTheme = theme.requested ?? s.preferences.theme;
  const closeBehavior = close.requested ?? s.preferences.closeBehavior ?? 'background';
  const startupDisabled = startup.reading || startup.saving || !startup.status || ['unknown', 'unavailable'].includes(startup.status.state);
  const startupError = startup.error || startup.readError;
  const startupText = startup.saving ? '正在保存启动设置…' : startup.reading ? '正在读取启动设置…' : startupError || startup.status?.reason || startup.feedback;
  const description = s.os === 'linux' ? '登录系统后自动最小化运行，可从任务栏或窗口切换返回。' : s.os === 'macos' ? '登录系统后自动在菜单栏运行，不打开主窗口。' : '登录系统后自动在托盘运行，不打开主窗口。';
  const closeDescription = s.os === 'linux' ? '关闭后最小化，保留窗口切换入口。' : s.os === 'macos' ? '关闭后保留连接和任务；没有其他工作窗口时，仅在屏幕顶部菜单栏保留入口。可从菜单栏重新打开或退出。' : '关闭后隐藏到托盘，已有连接和任务继续运行。';
  return `<div class="section-heading"><div><h2>通用</h2><p>应用外观、启动与窗口偏好。更改后立即保存。</p></div></div>
    <section class="general-settings-group" aria-labelledby="general-appearance"><h3 id="general-appearance" class="field-title">外观</h3><div class="themes" aria-describedby="theme-feedback">${[['system', '跟随系统'], ...themes.map(t => [t.id, t.name])].map(([id, label]) =>
      `<button id="theme-${id}" data-action="theme" data-id="${id}" aria-pressed="${selectedTheme === id}" aria-describedby="theme-feedback" class="theme-option ${selectedTheme === id ? 'selected' : ''}" ${theme.saving ? 'disabled' : ''}><div class="theme-preview ${id}"><i></i><div><span></span><span></span></div></div><span>${label}</span>${selectedTheme === id ? icon('check') : ''}</button>`,
    ).join('')}</div>${feedback('theme-feedback', theme.saving ? '正在保存主题…' : theme.error || '立即生效，重新打开应用后保留。', !!theme.error)}</section>
    <section class="general-settings-group" aria-labelledby="general-startup"><h3 id="general-startup" class="field-title">启动与窗口</h3>
    <div class="setting-row" data-setting-type="boolean"><div><label for="login-startup">开机启动并驻留托盘</label><p id="login-startup-description">${description}</p></div><label class="setting-switch"><input id="login-startup" type="checkbox" role="switch" aria-describedby="login-startup-description login-startup-feedback" ${(startup.saving ? startup.requested : startup.status?.enabled) ? 'checked' : ''} ${startupDisabled ? 'disabled' : ''}><span aria-hidden="true"></span></label></div>
    ${feedback('login-startup-feedback', startupText, !!startupError)}${!startup.reading && !startup.saving && (startupError || startup.status?.state === 'unknown') ? b('login-startup-retry', '重新读取', undefined, 'quiet') : ''}${s.os === 'macos' && !startup.reading && !startup.saving && startup.status?.state === 'stale' ? b('login-startup-update', '更新登录项', undefined, 'quiet') : ''}
    <div class="setting-row"><label for="close-behavior">关闭主窗口时</label><select id="close-behavior" aria-describedby="close-behavior-description close-behavior-feedback" ${close.saving ? 'disabled' : ''}><option value="background" ${closeBehavior === 'background' ? 'selected' : ''}>在后台继续运行（推荐）</option><option value="exit" ${closeBehavior === 'exit' ? 'selected' : ''}>退出应用</option></select><p id="close-behavior-description">${closeDescription} 从托盘菜单选择“退出 SoDeck”可结束应用。</p></div>${feedback('close-behavior-feedback', close.saving ? '正在保存窗口设置…' : close.error, !!close.error)}</section>`;
}
const pair = (name: string, value: unknown) => `<div class="pair"><dt>${name}</dt><dd>${e(value)}</dd></div>`;
function about(s: Snapshot) {
  return `<div class="section-heading"><div><h2>应用与数据</h2><p>应用信息和本地数据位置。</p></div></div><div class="about-brand"><div class="brand-mark">S</div><div><h3>SoDeck</h3><p>面向运维的远程服务器工作台</p></div></div><dl>${pair('应用版本', s.version)}${pair('构建目标', s.target)}${pair('数据目录', s.dataDirectory)}${pair('日志目录', s.logDirectory)}</dl><div class="actions">${b('usage-help', '使用帮助', 'info')}${b('copy-data', '复制数据路径', 'copy')}${b('copy-logs', '复制日志路径', 'copy', 'quiet')}</div><p class="hint">插件文件保留在你选择的外部目录。卸载或覆盖安装核心不会主动清理这些数据。</p>`;
}
