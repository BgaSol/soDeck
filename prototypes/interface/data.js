// Synthetic data only. No production application data or network is accessed.
export const networkInterfaces = [
  { id: 'eth0', addresses: ['10.20.0.24/24'], received: '48.2 KiB/s', sent: '2.4 KiB/s' },
  { id: 'lo', addresses: ['127.0.0.1/8'], received: '0 B/s', sent: '0 B/s' },
];
export const servers = [
  { id: 'web', name: '生产 Web', host: '10.20.1.24', user: 'ops', port: 22, tags: ['生产环境', 'Web'], auth: 'privateKey', route: [{ id: 'web-1', kind: 'socks5', proxyId: 'proxy' }, { id: 'web-2', kind: 'ssh', serverId: 'jump' }, { id: 'web-3', kind: 'ssh', serverId: 'zone' }], state: 'connected' },
  { id: 'db', name: '生产数据库', host: '10.20.2.12', user: 'ops', port: 22, tags: ['生产环境', '数据库'], auth: 'privateKey', route: [{ id: 'db-1', kind: 'socks5', proxyId: 'proxy' }, { id: 'db-2', kind: 'ssh', serverId: 'jump' }, { id: 'db-3', kind: 'ssh', serverId: 'zone' }], state: 'idle' },
  { id: 'edge', name: '边缘网关', host: '203.0.113.86', user: 'admin', port: 2222, tags: ['生产环境', '边缘'], auth: 'password', route: [{ id: 'edge-1', kind: 'ssh', serverId: 'jump' }], state: 'idle' },
  { id: 'stage', name: '预发布环境', host: '198.51.100.18', user: 'deploy', port: 22, tags: ['预发布环境', 'Web'], auth: 'privateKey', route: [], state: 'connected' },
  { id: 'test', name: '测试服务器', host: '10.30.0.42', user: 'dev', port: 22, tags: ['开发环境'], auth: 'password', route: [{ id: 'test-1', kind: 'ssh', serverId: 'stage' }], state: 'idle' },
  { id: 'offline', name: '旧版服务', host: '192.0.2.60', user: 'root', port: 22, tags: ['开发环境'], auth: 'password', route: [], state: 'failed' },
  { id: 'jump', name: '生产跳板', host: '203.0.113.8', user: 'ops', port: 22, tags: ['基础设施', '跳板'], auth: 'privateKey', route: [{ id: 'jump-1', kind: 'socks5', proxyId: 'proxy' }], state: 'idle' },
  { id: 'zone', name: '内网跳板', host: '10.20.0.5', user: 'ops', port: 22, tags: ['基础设施', '跳板'], auth: 'privateKey', route: [{ id: 'zone-1', kind: 'socks5', proxyId: 'proxy' }, { id: 'zone-2', kind: 'ssh', serverId: 'jump' }], state: 'idle' },
];
export const proxies = [
  { id: 'proxy', protocol: 'socks5', name: '办公网络代理', host: '127.0.0.1', port: 1080, tags: ['基础设施'], state: 'idle', auth: '无认证', route: [] },
];
export const tags = ['生产环境', '预发布环境', '开发环境', '基础设施', '跳板', 'Web', '数据库', '边缘'];
export const files = [
  { name: 'conf.d', type: 'folder' }, { name: 'sites-available', type: 'folder' },
  { name: 'sites-enabled', type: 'folder' }, { name: 'nginx.conf', type: 'file' },
  { name: 'mime.types', type: 'file' }, { name: 'fastcgi_params', type: 'file' },
];
export const nginx = [
  'user www-data;', 'worker_processes auto;', '',
  'events {', '    worker_connections 1024;', '}', '',
  'http {', '    include /etc/nginx/mime.types;', '    default_type application/octet-stream;', '',
  '    sendfile on;', '    keepalive_timeout 65;', '    gzip on;', '',
  '    server {', '        listen 80;', '        server_name example.test;', '',
  '        location / {', '            proxy_pass http://127.0.0.1:3000;', '            proxy_set_header Host $host;',
  '            proxy_set_header X-Real-IP $remote_addr;', '        }', '    }', '}',
].join('\n');
export const containers = [
  { id: 'api', name: 'shop-api-1', image: 'ghcr.io/example/shop-api:1.8.2', state: 'running', health: 'unhealthy', port: '127.0.0.1:3000 → 3000/tcp', cpu: '12.4%', memory: '284 MiB' },
  { id: 'web', name: 'shop-web-1', image: 'nginx:1.27-alpine', state: 'running', health: 'healthy', port: '0.0.0.0:80 → 80/tcp', cpu: '0.3%', memory: '24 MiB' },
  { id: 'db', name: 'shop-postgres-1', image: 'postgres:16-alpine', state: 'running', health: 'healthy', port: '5432/tcp', cpu: '2.1%', memory: '412 MiB' },
  { id: 'worker', name: 'shop-worker-1', image: 'ghcr.io/example/shop-api:1.8.2', state: 'stopped', health: '', port: '未发布', cpu: '—', memory: '—' },
];
export const plugins = [
  { id: 'terminal', name: '终端', icon: 'terminal', description: '交互式终端、输出搜索和多页会话。', installed: true, enabled: true },
  { id: 'files', name: '文件', icon: 'folder', description: '浏览与传输远端文件，编辑小文本。', installed: true, enabled: true },
  { id: 'network', name: '网络隧道', icon: 'network', description: '本机 TCP 转发和 SOCKS5 入口。', installed: true, enabled: true },
  { id: 'system', name: '系统状态', icon: 'activity', description: 'CPU、内存、网络和磁盘指标。', installed: true, enabled: true },
  { id: 'docker', name: 'Docker', icon: 'box', description: '容器状态、详情、日志和日常操作。', installed: true, enabled: true },
];
export const messages = [
  { id: 'm1', title: 'shop-api-1 健康检查失败', text: '最近一次检查未通过。查看容器日志，核对服务监听地址。', server: '生产 Web', source: 'Docker', time: '14:32:08', type: 'error', read: false },
  { id: 'm2', title: '文件上传完成', text: 'site.conf 已上传到 /etc/nginx/conf.d，共 1 个文件。', server: '生产 Web', source: '文件', time: '14:29:41', type: 'success', read: false },
  { id: 'm3', title: '预发布环境已连接', text: '已建立 deploy@198.51.100.18:22 会话。', server: '预发布环境', source: '核心', time: '14:25:16', type: 'info', read: true },
];
export const logs = [
  { time: '14:32:08', source: 'Docker', level: 'warn', text: '容器健康状态已更新：shop-api-1 / unhealthy' },
  { time: '14:29:41', source: '文件', level: 'info', text: '上传批次结束：1 项成功，0 项失败' },
  { time: '14:25:16', source: '核心', level: 'info', text: '连接已建立：预发布环境' },
  { time: '14:24:50', source: '网络隧道', level: 'info', text: '本机监听已创建：127.0.0.1:1080' },
  { time: '14:20:12', source: '核心', level: 'info', text: '插件启动完成：sodeck.docker' },
];
export const stateLabel = { connected: '已连接', idle: '未连接', failed: '连接失败', disconnected: '已断开', connecting: '连接中' };
