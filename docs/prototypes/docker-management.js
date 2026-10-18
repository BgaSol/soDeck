/* Interaction model for design review only. No IPC, network requests or Docker commands. */
const icons = {
  boxes:
    '<path d="m12 3 4 2.3v4.6l-4 2.3-4-2.3V5.3L12 3Z M8 5.3l4 2.3 4-2.3 M12 7.6v4.6 M8 10l-4 2.3v4.6l4 2.3 4-2.3v-4.6 M4 12.3l4 2.3 4-2.3 M8 14.6v4.6 M16 10l4 2.3v4.6l-4 2.3-4-2.3 M12 12.3l4 2.3 4-2.3 M16 14.6v4.6"/>',
  box: '<path d="m12 3 9 5v8l-9 5-9-5V8l9-5Z M3 8l9 5 9-5 M12 13v8 M7.5 5.5l9 5"/>',
  terminal:
    '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3 M13 15h4"/>',
  folder:
    '<path d="M3 7V5a1 1 0 0 1 1-1h5l2 3h9a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1Z"/>',
  file: '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z M14 2v6h6 M8 12h8 M8 16h6"/>',
  activity: '<path d="M2 12h5l3-9 4 18 3-9h5"/>',
  network:
    '<rect x="9" y="2" width="6" height="6" rx="1"/><rect x="2" y="16" width="6" height="6" rx="1"/><rect x="16" y="16" width="6" height="6" rx="1"/><path d="M12 8v5 M5 16v-3h14v3"/>',
  refresh:
    '<path d="M20 7v5h-5 M4 17v-5h5 M5.5 7a7 7 0 0 1 11.6-2L20 8 M4 16l2.9 3A7 7 0 0 0 18.5 17"/>',
  search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 4.5 4.5"/>',
  more: '<circle cx="5" cy="12" r=".9"/><circle cx="12" cy="12" r=".9"/><circle cx="19" cy="12" r=".9"/>',
  back: '<path d="m14 6-6 6 6 6 M8 12h12"/>',
  chevron: '<path d="m9 5 7 7-7 7"/>',
  plus: '<path d="M12 5v14 M5 12h14"/>',
  up: '<path d="m5 12 7-7 7 7 M12 5v15"/>',
  upload:
    '<path d="m7 8 5-5 5 5 M12 3v12 M4 14v5a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-5"/>',
  logs: '<path d="M7 3h11a2 2 0 0 1 2 2v15H9a2 2 0 0 1-2-2V3Z M7 3H4a2 2 0 0 0-2 2v3h5 M11 7h5 M11 11h5 M11 15h3"/>',
  history: '<path d="M3 5v5h5 M3.5 9a9 9 0 1 1 .5 8 M12 7v5l3 2"/>',
  clear: '<path d="m15 3 6 6-11 11H6l-4-4L15 3Z M7 11l7 7 M10 20h12"/>',
  stop: '<rect x="5" y="5" width="14" height="14" rx="2"/>',
  play: '<path d="m8 4 12 8-12 8Z"/>',
  pause: '<path d="M8 4v16 M16 4v16"/>',
  copy: '<rect x="8" y="8" width="12" height="13" rx="2"/><path d="M16 8V4a1 1 0 0 0-1-1H4a1 1 0 0 0-1 1v12a1 1 0 0 0 1 1h4"/>',
  check: '<path d="m5 12 4 4L19 6"/>',
  close: '<path d="m6 6 12 12 M6 18 18 6"/>',
  trash: '<path d="M3 6h18 M9 6V3h6v3 M5 6l1 15h12l1-15 M10 10v7 M14 10v7"/>',
  disk: '<ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v14c0 4 16 4 16 0V5 M4 12c0 4 16 4 16 0"/>',
  layers:
    '<path d="m12 3 10 5-10 5L2 8l10-5Z M2 12l10 5 10-5 M2 16l10 5 10-5"/>',
  eye: '<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/>',
  alert: '<path d="m12 3 10 18H2L12 3Z M12 9v5 M12 17h.01"/>',
};
const icon = (name, cls = "") =>
  `<svg class="${cls}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${icons[name] || icons.box}</svg>`;
document.querySelectorAll("[data-icon]").forEach((el) =>
  el.replaceWith(
    Object.assign(document.createElement("span"), {
      innerHTML: icon(el.dataset.icon),
    }),
  ),
);
const $ = (selector) => document.querySelector(selector);
const escapeHtml = (value) =>
  String(value).replace(
    /[&<>"']/g,
    (char) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        char
      ],
  );
const clone = (value) => JSON.parse(JSON.stringify(value));
const initialContainers = [
  {
    id: "a18c2d4e6f90",
    name: "api-gateway",
    image: "nginx:1.27-alpine",
    state: "running",
    health: "healthy",
    project: "storefront",
    age: "3 天",
    cpu: "0.8%",
    memory: "42 / 512 MiB",
    ports: 2,
  },
  {
    id: "b84d2e9f1a30",
    name: "order-service",
    image: "company/orders:2.4",
    state: "running",
    health: "unhealthy",
    project: "storefront",
    age: "3 天",
    cpu: "12.4%",
    memory: "286 / 512 MiB",
    ports: 1,
  },
  {
    id: "c9e1a2d3b4f5",
    name: "postgres",
    image: "postgres:16",
    state: "running",
    health: "healthy",
    project: "storefront",
    age: "3 天",
    cpu: "1.2%",
    memory: "384 / 1024 MiB",
    ports: 1,
  },
  {
    id: "d2f4e1a6c8b0",
    name: "redis",
    image: "redis:7-alpine",
    state: "running",
    health: "none",
    project: "storefront",
    age: "3 天",
    cpu: "0.1%",
    memory: "18 / 256 MiB",
    ports: 0,
  },
  {
    id: "e71a0b5c9d32",
    name: "nightly-backup",
    image: "company/backup:stable",
    state: "exited",
    health: "none",
    project: "",
    age: "2 小时",
    cpu: "—",
    memory: "—",
    ports: 0,
  },
  {
    id: "f43a8c6d2e01",
    name: "jobs-worker",
    image: "company/jobs:stable",
    state: "restarting",
    health: "none",
    project: "",
    age: "12 秒",
    cpu: "—",
    memory: "—",
    ports: 0,
  },
];
const initialImages = [
  {
    id: "sha256:7a9c21fd8b02",
    name: "nginx:1.27-alpine",
    size: "48.2 MB",
    used: ["api-gateway"],
  },
  {
    id: "sha256:7a9c21fd8b02",
    name: "nginx:stable-alpine",
    size: "48.2 MB",
    used: ["api-gateway"],
  },
  {
    id: "sha256:9e2a31bc4d80",
    name: "company/orders:2.4",
    size: "186 MB",
    used: ["order-service"],
  },
  {
    id: "sha256:3f12a6c94b21",
    name: "postgres:16",
    size: "432 MB",
    used: ["postgres"],
  },
  {
    id: "sha256:62e1fa3345db",
    name: "redis:7-alpine",
    size: "41.6 MB",
    used: ["redis"],
  },
  {
    id: "sha256:8d3e514c9a02",
    name: "company/orders:2.3",
    size: "182 MB",
    used: [],
  },
];
let containers = clone(initialContainers),
  images = clone(initialImages);
let view = {
  page: "containers",
  id: null,
  query: "",
  filter: "all",
  service: "docker",
};
let tasks = [],
  scenario = "normal",
  modalType = null,
  modalData = null,
  menuTrigger = null,
  menuReturnFocus = null,
  dialogTrigger = null;
let navigationFrame = 0,
  position = 0,
  velocity = 0,
  serial = 0,
  resetGeneration = 0;
const stateNames = {
  running: "运行中",
  exited: "已停止",
  paused: "已暂停",
  restarting: "重启中",
};
const btn = (text, action, options = "") =>
  `<button data-action="${action}" ${options}>${text}</button>`;
const more = (kind, id, label = "更多操作") =>
  btn(
    icon("more"),
    "menu",
    `class="icon row-menu" data-kind="${kind}" data-id="${escapeHtml(id)}" aria-label="${escapeHtml(label)}" aria-haspopup="menu" aria-expanded="false"`,
  );
const copy = (text) =>
  btn(
    icon("copy"),
    "copy",
    `class="icon" data-copy="${escapeHtml(text)}" aria-label="复制 ${escapeHtml(text)}"`,
  );
const fact = (label, content) => `<dt>${label}</dt><dd>${content}</dd>`;
const disclosure = (title, count, content, open = false) =>
  `<details class="disclosure" ${open ? "open" : ""}><summary>${title}<small>${count}</small>${icon("chevron", "chevron")}</summary><div class="disclosure-body">${content}</div></details>`;
const row = (kind, id, title, subtitle, status, tone = "", symbol = "box") =>
  `<div class="object-row" role="listitem"><button class="row-main" data-action="select" data-kind="${kind}" data-id="${escapeHtml(id)}">${kind === "containers" ? `<i class="dot ${tone}"></i>` : icon(symbol)}<span class="row-text"><strong>${escapeHtml(title)}</strong><small>${escapeHtml(subtitle)}</small></span><span class="row-status ${tone}">${status}</span></button>${more(kind, id, `${title} 的更多操作`)}</div>`;
const cById = (id) => containers.find((c) => c.id === id);
const iByName = (id) => images.find((i) => i.name === id);
const activeFor = (id) =>
  tasks.find(
    (t) => t.target === id && ["running", "unknown"].includes(t.state),
  );
function announce(message) {
  $("#announce").textContent = message;
}
function springNavigate(direction = 1) {
  cancelAnimationFrame(navigationFrame);
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    $("#page").style.transform = "";
    return;
  }
  position += direction * 8;
  let previous = performance.now();
  const step = (now) => {
    const dt = Math.min((now - previous) / 1000, 0.032);
    previous = now;
    velocity += (-500 * position - 44.72 * velocity) * dt;
    position += velocity * dt;
    $("#page").style.transform = `translateX(${position}px)`;
    if (Math.abs(position) > 0.05 || Math.abs(velocity) > 0.05)
      navigationFrame = requestAnimationFrame(step);
    else {
      position = velocity = 0;
      $("#page").style.transform = "";
    }
  };
  navigationFrame = requestAnimationFrame(step);
}
function navigate(page, id = null) {
  const returning = !!view.id && !id;
  closeMenu(false);
  view = { ...view, page, id, service: "docker" };
  render();
  springNavigate(returning ? -1 : 1);
  if (id) $("#page .back")?.focus({ preventScroll: true });
}
function renderHead() {
  const offline = scenario === "offline";
  const running = containers.filter((c) => c.state === "running").length;
  const count = scenario === "empty" ? 0 : containers.length;
  $("#engine-head").innerHTML =
    `<div class="engine-head"><div class="engine-line">${icon("boxes")}<strong>Docker</strong><span class="engine-status ${offline ? "warning" : "running"}">${offline ? "连接中断" : "可访问"}</span><div class="engine-tools">${btn(icon("refresh"), "refresh", 'class="icon" aria-label="刷新 Docker 状态"')}${more("engine", "engine", "Docker 更多功能")}</div></div><p class="endpoint mono" title="unix:///var/run/docker.sock">unix:///var/run/docker.sock</p><div class="engine-meta"><span>${count} 个容器 · ${scenario === "empty" ? 0 : running} 运行${offline ? " · 最后数据" : ""}</span><span class="updated">${offline ? "更新于 09:41" : "刚刚更新"}</span></div></div>`;
}
function navSegments() {
  return `<div class="segments" role="group" aria-label="Docker 对象类型">${[
    ["containers", "容器", containers.length],
    ["images", "镜像", images.length],
    ["projects", "项目", 1],
  ]
    .map(([key, name, count]) =>
      btn(
        `${name}<span class="count">${scenario === "empty" ? 0 : count}</span>`,
        "page",
        `data-page="${key}" aria-pressed="${view.page === key}"`,
      ),
    )
    .join("")}</div>`;
}
function renderList() {
  const names = { containers: "容器", images: "镜像", projects: "项目" };
  const name = names[view.page];
  const createAction = {
    containers: "create",
    images: "pull",
    projects: "associate",
  }[view.page];
  const createLabel = {
    containers: "创建容器",
    images: "拉取镜像",
    projects: "关联项目",
  }[view.page];
  const controls = `<div class="object-controls">${navSegments()}<label class="search">${icon("search")}<input id="object-search" aria-label="搜索${name}" placeholder="搜索${name === "容器" ? "名称、镜像或 ID" : name}" value="${escapeHtml(view.query)}" autocomplete="off" /></label><div class="filter-bar">${view.page === "containers" ? `<select id="state-filter" aria-label="容器状态筛选"><option value="all">全部状态</option><option value="running" ${view.filter === "running" ? "selected" : ""}>运行中</option><option value="attention" ${view.filter === "attention" ? "selected" : ""}>需要关注</option><option value="exited" ${view.filter === "exited" ? "selected" : ""}>已停止</option></select>` : `<span class="small muted">${view.page === "images" ? "按镜像名称" : "当前引擎中的项目"}</span>`}${btn(`${icon("plus")}${createLabel}`, createAction, `class="${view.page === "images" ? "primary" : "quiet"}" ${scenario === "offline" ? "disabled" : ""}`)}</div></div>`;
  let rows = "";
  const query = view.query.toLowerCase();
  if (scenario !== "empty") {
    if (view.page === "containers")
      rows = containers
        .filter(
          (c) =>
            `${c.name} ${c.image} ${c.id}`.toLowerCase().includes(query) &&
            (view.filter === "all" ||
              (view.filter === "attention"
                ? c.health === "unhealthy" || c.state === "restarting"
                : c.state === view.filter)),
        )
        .map((c) => {
          const task = activeFor(c.id);
          const tone =
            c.health === "unhealthy" || c.state === "restarting"
              ? "warning"
              : c.state === "running"
                ? "running"
                : "";
          return row(
            "containers",
            c.id,
            c.name,
            c.image,
            task
              ? `${task.state === "unknown" ? "待确认" : "操作中"}`
              : c.health === "unhealthy"
                ? "不健康"
                : stateNames[c.state],
            tone,
          );
        })
        .join("");
    if (view.page === "images")
      rows = images
        .filter((i) => `${i.name} ${i.id}`.toLowerCase().includes(query))
        .map((i) =>
          row(
            "images",
            i.name,
            i.name,
            i.used.length ? `${i.used.length} 个容器引用` : "未被容器引用",
            i.size,
            "",
            "layers",
          ),
        )
        .join("");
    if (view.page === "projects" && "storefront".includes(query))
      rows = row(
        "projects",
        "storefront",
        "storefront",
        "/srv/storefront/compose.yaml",
        '4 个服务<br><span class="warning">1 需关注</span>',
        "",
        "boxes",
      );
  }
  const offline =
    scenario === "offline"
      ? '<div class="notice neutral" style="margin-inline:14px">保留上次列表。连接恢复后可继续操作。</div>'
      : "";
  const empty = `<div class="empty">${icon(view.page === "images" ? "layers" : "boxes")}<strong>${query || view.filter !== "all" ? "没有匹配的对象" : `还没有${name}`}</strong><p>${query || view.filter !== "all" ? "换个关键词，或清除筛选。" : view.page === "images" ? "拉取一个镜像，开始部署服务。" : view.page === "projects" ? "关联服务器上的 Compose 配置。" : "从已有镜像创建第一个容器。"}</p>${btn(query || view.filter !== "all" ? "清除筛选" : createLabel, query || view.filter !== "all" ? "clear-filter" : createAction, 'class="primary"')}</div>`;
  $("#page").innerHTML =
    `${controls}${offline}${rows ? `<div class="object-list" role="list" aria-label="${name}列表">${rows}</div><div class="list-note">${view.page === "containers" ? `${icon("alert")} ${containers.filter((c) => c.health === "unhealthy" && c.state === "running").length} 个健康异常 · ${containers.filter((c) => c.state === "restarting").length} 个正在重启` : view.page === "images" ? "同一镜像的不同 Tag 分别显示。" : "更新镜像与重启服务分别操作。"}</div>` : empty}`;
}
function noticeFor(c) {
  const task = [...tasks].reverse().find((t) => t.target === c.id);
  if (scenario === "offline")
    return '<div class="notice neutral">连接已中断，显示最后一次获取的详情。</div>';
  if (task?.state === "unknown")
    return `<div class="notice"><strong>操作结果待确认</strong><p>连接在执行中断开，容器可能已发生变化。</p>${btn("核对当前状态", "reconcile", `class="link" data-task="${task.id}"`)}</div>`;
  if (task?.state === "failed")
    return `<div class="notice error" role="alert"><strong>${task.label}未完成</strong><p>当前账号没有执行此操作的权限。</p>${btn("查看详情", "task", `class="link" data-task="${task.id}"`)}</div>`;
  if (c.health === "unhealthy")
    return `<div class="notice"><strong>健康检查连续失败 3 次</strong><p>GET /health 返回 503。容器进程仍在运行。</p>${btn("查看日志", "logs", `class="link" data-id="${c.id}"`)}</div>`;
  if (c.state === "restarting")
    return '<div class="notice"><strong>容器反复重启</strong><p>最近退出码 1 · 已重启 8 次。查看日志以定位启动失败原因。</p></div>';
  return "";
}
function renderContainer() {
  const c = cById(view.id);
  if (!c) {
    navigate("containers");
    return;
  }
  const busy = activeFor(c.id),
    disabled = busy || scenario === "offline" ? "disabled" : "";
  const attr = `data-id="${c.id}" ${disabled}`;
  const primary =
    c.state === "exited"
      ? btn(`${icon("play")}启动`, "start", `${attr} class="primary"`)
      : c.state === "paused"
        ? btn(`${icon("play")}恢复`, "unpause", `${attr} class="primary"`)
        : btn(`${icon("stop")}停止`, "stop", attr);
  const health =
    c.health === "healthy"
      ? '<span class="running">健康</span>'
      : c.health === "unhealthy"
        ? '<span class="warning">不健康</span>'
        : '<span class="muted">未配置健康检查</span>';
  const ports = c.ports
    ? `<div class="info-row"><div><strong class="mono">0.0.0.0:8080 → 80/tcp</strong><p>已发布到宿主机</p></div>${btn("转发", "forward", 'class="link"')}</div>${c.ports > 1 ? '<div class="info-row"><div><strong class="mono">[::]:8443 → 443/tcp</strong><p>已发布到宿主机 · IPv6</p></div></div>' : ""}`
    : '<p class="small muted">未发布宿主机端口。</p>';
  $("#page").innerHTML =
    `<div class="detail">${btn(`${icon("back")}返回容器`, "back", 'class="link back"')}<div class="object-title"><h2>${escapeHtml(c.name)}</h2></div><p class="detail-subtitle">${escapeHtml(c.image)}</p><div class="status-line"><i class="dot ${c.state === "running" ? "running" : c.state === "restarting" ? "warning" : ""}"></i><span>${stateNames[c.state]}</span><span class="separator">·</span>${c.state === "running" ? health : `<span class="muted">${c.age === "刚刚" ? "刚刚" : c.age + "前"}${c.state === "exited" ? "退出" : "更新"}</span>`}</div><div class="action-bar">${primary}${c.state !== "exited" ? btn(`${icon("refresh")}重启`, "restart", attr) : ""}${btn(`${icon("logs")}日志`, "logs", `data-id="${c.id}" ${scenario === "offline" ? "disabled" : ""}`)}${btn(icon("terminal"), "terminal", `class="wide-action" aria-label="进入容器终端" data-id="${c.id}" ${c.state !== "running" ? "disabled" : ""}`)}${more("containers", c.id)}</div>${busy?.state === "running" ? `<div class="notice neutral">${busy.label}… 其他容器仍可操作。</div>` : noticeFor(c)}<section class="fact-section"><div class="section-head"><h3>概览</h3>${c.project ? btn(c.project, "project-detail", 'class="link"') : '<span class="small muted">独立容器</span>'}</div><dl class="facts">${fact("容器 ID", `<span class="inline-copy mono">${c.id}${copy(c.id)}</span>`)}${fact("已运行", c.state === "running" ? c.age : "—")}${fact("重启策略", `<span>${c.policy || (c.project ? "unless-stopped" : "no")}</span>`)}${fact("最近退出", c.state === "exited" ? "退出码 0 · 正常结束" : c.state === "restarting" ? "退出码 1 · 非正常结束" : "—")}${c.health === "unhealthy" ? fact("重启次数", "0") : ""}</dl>${c.state === "running" ? `<div class="metrics"><div class="metric"><div class="metric-line"><span>CPU</span><strong>${c.cpu}</strong></div></div><div class="metric"><div class="metric-line"><span>内存</span><strong>${c.memory}</strong></div><div class="track"><span style="width:${c.health === "unhealthy" ? 56 : 8}%"></span></div></div></div>` : ""}</section>${disclosure("端口与网络", `${c.ports} 个端口 · 1 个网络`, `${ports}<div class="info-row"><div><strong>storefront_default</strong><p class="mono">172.20.0.2 · bridge</p></div>${copy("172.20.0.2")}</div>`, true)}${disclosure("存储", "2 个挂载", `<div class="info-row"><div><strong>配置文件</strong><p class="mono">/srv/storefront/config → /etc/nginx/conf.d</p><p>Bind Mount · 只读</p></div>${btn("定位", "locate", 'class="link"')}</div><div class="info-row"><div><strong>storefront_logs</strong><p class="mono">/var/log/nginx</p><p>命名卷 · 读写</p></div>${btn("详情", "volume", 'class="link"')}</div>`)}${disclosure("配置", "命令、环境变量与标签", `<dl class="facts">${fact("命令", '<span class="mono">nginx -g daemon off;</span>')}${fact("工作目录", '<span class="mono">/</span>')}${fact("用户", "默认用户")}${fact("环境变量", `<span class="inline-copy mono">APP_ENV　<span id="env-value">••••••</span>${btn(icon("eye"), "reveal-env", 'class="icon" aria-label="显示 APP_ENV 的值" aria-pressed="false"')}</span>`)}</dl><div style="margin-top:10px">${btn("查看完整配置", "inspect", `class="link" data-id="${c.id}"`)}</div>`)}${disclosure("进程与资源", "按需查看", `<dl class="facts">${fact("进程数", "4")}${fact("网络累计", "接收 1.2 GiB / 发送 3.6 GiB")}${fact("磁盘 I/O", "读取 28 MiB / 写入 12 MiB")}</dl>`)}<p class="small muted" style="padding-top:12px">${c.project ? "此容器由 Compose 管理。持久配置请在项目中修改。" : "独立容器 · 配置仅应用于当前容器。"}</p></div>`;
}
function renderImage() {
  const image = iByName(view.id);
  if (!image) {
    navigate("images");
    return;
  }
  const tags = images.filter((i) => i.id === image.id);
  $("#page").innerHTML =
    `<div class="detail">${btn(`${icon("back")}返回镜像`, "back", 'class="link back"')}<div class="object-title"><h2>${escapeHtml(image.name)}</h2></div><p class="detail-subtitle">${image.size} · linux/amd64</p><div class="action-bar">${btn(`${icon("play")}创建容器`, "create", `class="primary" data-image="${escapeHtml(image.name)}"`)}${btn("拉取更新", "pull", `data-image="${escapeHtml(image.name)}"`)}${more("images", image.name)}</div><section class="fact-section"><div class="section-head"><h3>镜像信息</h3></div><dl class="facts">${fact("ID", `<span class="inline-copy mono">${image.id}${copy(image.id)}</span>`)}${fact("创建时间", "2026-10-12 08:30")}${fact("平台", "Linux / amd64")}${fact("大小", image.size)}</dl></section><section class="fact-section"><div class="section-head"><h3>标签</h3><span class="small muted">${tags.length} 个引用</span></div>${tags.map((i) => `<div class="info-row"><div class="mono">${escapeHtml(i.name)}</div>${copy(i.name)}</div>`).join("")}${btn("添加 Tag", "tag", `class="link" data-image="${escapeHtml(image.name)}"`)}</section><section class="fact-section"><div class="section-head"><h3>引用容器</h3><span class="small muted">${image.used.length} 个</span></div>${image.used.length ? image.used.map((name) => `<div class="info-row"><div>${escapeHtml(name)}</div>${btn("查看", "select", `class="link" data-kind="containers" data-id="${containers.find((c) => c.name === name)?.id || ""}"`)}</div>`).join("") : '<p class="small muted">没有容器引用此镜像。</p>'}</section><p class="small muted" style="padding-top:12px">拉取更新后，已有容器继续使用原镜像。更新服务请在项目中执行。</p></div>`;
}
function renderProject() {
  $("#page").innerHTML =
    `<div class="detail">${btn(`${icon("back")}返回项目`, "back", 'class="link back"')}<h2>storefront</h2><p class="detail-subtitle mono">/srv/storefront/compose.yaml</p><div class="status-line"><i class="dot running"></i>${containers.filter((c) => c.project && c.state === "running").length} 个服务运行中<span class="separator">·</span><span class="muted">${containers.filter((c) => c.project && c.health === "unhealthy" && c.state === "running").length} 需关注</span></div><div class="action-bar">${btn("停止", "project-stop")}${btn("重启", "project-restart")}${btn("日志", "project-logs")}${more("projects", "storefront")}</div><section class="fact-section"><div class="section-head"><h3>服务</h3>${btn("更新部署", "update", 'class="link"')}</div>${containers
      .filter((c) => c.project)
      .map(
        (c) =>
          `<div class="project-service">${row("containers", c.id, c.name, c.image, c.health === "unhealthy" ? "不健康" : stateNames[c.state], c.health === "unhealthy" ? "warning" : "running")}</div>`,
      )
      .join(
        "",
      )}</section>${disclosure("项目配置", "1 个文件", `<dl class="facts">${fact("目录", '<span class="mono">/srv/storefront</span>')}${fact("环境文件", '<span class="mono">.env</span>')}${fact("Profile", "默认")}</dl>${btn("编辑 compose.yaml", "compose-file", 'class="link" style="margin-top:8px"')}`, true)}<p class="small muted" style="padding-top:10px">重启保留当前配置。应用配置或新镜像，请选择“更新部署”。</p></div>`;
}
function renderResources() {
  $("#page").innerHTML =
    `<div class="detail">${btn(`${icon("back")}返回容器`, "resources-back", 'class="link back"')}<h2>空间与资源</h2><p class="detail-subtitle">当前 Docker 引擎</p><div class="resource-title"><strong>8.42 <span style="font-size:13px;font-weight:400">GB</span></strong><span class="small muted">约 1.24 GB 可回收</span></div><div class="track"><span style="width:74%"></span></div><div style="margin-top:12px">${[
      ["镜像", "6 个引用 · 5 个镜像", "2.18 GB", "layers"],
      ["容器", "6 个 · 4 个运行", "240 MB", "box"],
      ["卷", "3 个 · 2 个使用中", "5.46 GB", "disk"],
      ["构建缓存", "8 项", "540 MB", "layers"],
    ]
      .map(
        ([name, n, size, symbol]) =>
          `<div class="resource-item">${icon(symbol)}<div><strong>${name}</strong><p>${n}</p></div><span>${size}</span></div>`,
      )
      .join(
        "",
      )}</div><div class="action-bar">${btn("查看清理候选", "cleanup", 'class="primary"')}</div>${disclosure("卷", "3 个", `<div class="info-row"><div><strong>storefront_data</strong><p>postgres 使用中 · 4.8 GB</p></div>${btn("详情", "volume", 'class="link"')}</div><div class="info-row"><div><strong>storefront_logs</strong><p>api-gateway 使用中 · 660 MB</p></div>${btn("详情", "volume", 'class="link"')}</div>${btn("创建卷", "new-volume", 'class="link"')}`)}${disclosure("网络", "2 个", `<div class="info-row"><div><strong>storefront_default</strong><p>bridge · 4 个容器</p></div>${btn("详情", "network-detail", 'class="link"')}</div>${btn("创建网络", "new-network", 'class="link"')}`)}<p class="small muted" style="padding-top:12px">可回收空间为估计值。共享层不会重复计算为释放空间。</p></div>`;
}
function renderMonitor() {
  $("#page").innerHTML =
    `<div class="detail"><h2 style="margin-block:16px">服务器监控</h2>${[
      ["CPU", "12.8%", 13],
      ["内存", "3.0 / 8.0 GiB", 38],
      ["磁盘", "42 / 120 GB", 35],
    ]
      .map(
        ([name, value, width]) =>
          `<section class="fact-section"><div class="section-head"><h3>${name}</h3><span>${value}</span></div><div class="track"><span style="width:${width}%"></span></div></section>`,
      )
      .join(
        "",
      )}<p class="small muted" style="margin-top:14px">Production · 刚刚更新</p></div>`;
}
function render() {
  renderHead();
  document
    .querySelectorAll(".service-navigation [role=tab]")
    .forEach((b) =>
      b.setAttribute(
        "aria-selected",
        String(b.dataset.action === view.service),
      ),
    );
  $("#engine-head").hidden = view.service !== "docker";
  if (view.service === "monitor") renderMonitor();
  else if (view.page === "resources") renderResources();
  else if (!view.id) renderList();
  else if (view.page === "containers") renderContainer();
  else if (view.page === "images") renderImage();
  else renderProject();
  renderTasks();
}
function renderTasks() {
  const task = tasks.at(-1);
  $("#tasks").hidden = !task;
  if (!task) return;
  $("#tasks").innerHTML =
    `<button class="task-button" data-action="task" data-task="${task.id}">${task.state === "running" ? '<i class="spinner"></i>' : icon(task.state === "success" ? "check" : "alert")}<span>${task.label}${{ running: "…", success: " · 已完成", failed: " · 未完成", unknown: " · 结果待确认" }[task.state]}</span><small>${tasks.length} 项</small>${icon("chevron")}</button>`;
}
function runTask(label, target, mutate, action = "") {
  if (activeFor(target)) {
    announce("此对象已有进行中的操作。");
    return;
  }
  const task = {
    id: ++serial,
    label,
    target,
    state: "running",
    mutate,
    action,
  };
  tasks.push(task);
  const currentScenario = scenario,
    generation = resetGeneration;
  closeDialog();
  render();
  announce(`${label}，正在执行。`);
  setTimeout(
    () => {
      if (generation !== resetGeneration) return;
      if (currentScenario === "failure") task.state = "failed";
      else if (currentScenario === "unknown") task.state = "unknown";
      else {
        task.state = "success";
        mutate?.();
      }
      const focusedAction = document.activeElement?.dataset.action;
      const previousScroll = $("#page .detail")?.scrollTop;
      render();
      if (previousScroll != null && $("#page .detail"))
        $("#page .detail").scrollTop = previousScroll;
      if (focusedAction)
        $("#page")
          .querySelector(`[data-action="${focusedAction}"]`)
          ?.focus({ preventScroll: true });
      if (modalType === "progress" && modalData?.id === task.id)
        showDialog("task", task);
      announce(
        `${label}${task.state === "success" ? "已完成" : task.state === "failed" ? "未完成" : "结果待确认"}`,
      );
    },
    action === "pull" ? 4200 : 1100,
  );
  return task;
}
function containerAction(action, id) {
  const c = cById(id || view.id);
  if (!c || scenario === "offline") return;
  const label = {
    start: "启动",
    stop: "停止",
    restart: "重启",
    pause: "暂停",
    unpause: "恢复",
    kill: "强制停止",
    remove: "删除",
  }[action];
  runTask(
    `${label} ${c.name}`,
    c.id,
    () => {
      if (action === "remove")
        containers = containers.filter((item) => item.id !== c.id);
      else {
        c.state = {
          start: "running",
          stop: "exited",
          restart: "running",
          pause: "paused",
          unpause: "running",
          kill: "exited",
        }[action];
        c.age = "刚刚";
      }
    },
    action,
  );
}
function closeMenu(restore = true) {
  $("#menu").hidden = true;
  menuTrigger?.setAttribute("aria-expanded", "false");
  if (restore && menuReturnFocus?.isConnected) menuReturnFocus.focus();
}
function showMenu(trigger, returnFocus = trigger) {
  menuReturnFocus = returnFocus;
  const { kind, id } = trigger.dataset;
  menuTrigger = trigger;
  const item = (text, action, extra = "", danger = false) =>
    btn(
      text,
      action,
      `${extra} role="menuitem" ${danger ? 'class="danger"' : ""}`,
    );
  let html = "";
  if (kind === "engine")
    html = `${item("空间与资源", "resources")}${item("操作记录", "all-tasks")}<hr>${item("创建容器…", "create")}${item("拉取镜像…", "pull")}${item("关联 Compose 项目…", "associate")}`;
  if (kind === "containers") {
    const c = cById(id),
      disabled = activeFor(id) || scenario === "offline" ? "disabled" : "",
      attrs = `data-id="${id}" ${disabled}`;
    html = `${c.state === "running" ? item(`${icon("terminal")}进入终端`, "terminal", `data-id="${id}"`) : ""}${item(`${icon("logs")}查看日志`, "logs", `data-id="${id}"`)}<hr>${c.state === "running" ? item("暂停", "pause", attrs) : c.state === "paused" ? item("恢复", "unpause", attrs) : c.state === "exited" ? item("启动", "start", attrs) : ""}${item("重命名…", "rename", attrs)}${item("重启策略…", "policy", attrs)}<hr>${c.state === "running" || c.state === "restarting" ? item("强制停止…", "confirm-kill", attrs, true) : ""}${item("删除容器…", "confirm-remove", attrs, true)}`;
  }
  if (kind === "images")
    html = `${item("创建容器…", "create", `data-image="${escapeHtml(id)}"`)}${item("拉取更新…", "pull", `data-image="${escapeHtml(id)}"`)}${item("添加 Tag…", "tag", `data-image="${escapeHtml(id)}"`)}<hr>${item("移除镜像引用…", "remove-image", `data-image="${escapeHtml(id)}"`, true)}`;
  if (kind === "projects")
    html = `${item("编辑 Compose 文件", "compose-file")}${item("更新部署…", "update")}${item("查看服务日志", "project-logs")}<hr>${item("停止并移除项目…", "down", "", true)}`;
  $("#menu").innerHTML = html;
  $("#menu").hidden = false;
  trigger.setAttribute("aria-expanded", "true");
  const rect = trigger.getBoundingClientRect(),
    menuRect = $("#menu").getBoundingClientRect();
  $("#menu").style.left =
    `${Math.max(8, Math.min(rect.right - menuRect.width, innerWidth - menuRect.width - 8))}px`;
  $("#menu").style.top =
    `${Math.max(8, rect.bottom + menuRect.height + 6 > innerHeight ? rect.top - menuRect.height - 5 : rect.bottom + 5)}px`;
  $("#menu").querySelector("button:not(:disabled)")?.focus();
}
function closeDialog() {
  if ($("#dialog").open) $("#dialog").close();
  modalType = null;
  modalData = null;
}
const field = (label, control, help = "") =>
  `<label class="form-field">${label}${control}${help ? `<span class="help">${help}</span>` : ""}</label>`;
const input = (name, value = "", attrs = "") =>
  `<input name="${name}" value="${escapeHtml(value)}" ${attrs} />`;
function showDialog(type, data = {}) {
  closeMenu(false);
  if (!$("#dialog").open) dialogTrigger = document.activeElement;
  modalType = type;
  modalData = data;
  const c = cById(data.id || view.id) || containers[0];
  let title = "",
    subtitle = "Production · 当前 Docker 引擎",
    body = "",
    primary = "",
    dangerous = false,
    wide = false;
  const targetSummary = c
    ? `<div class="object-summary"><dl class="facts">${fact("容器", escapeHtml(c.name))}${fact("ID", `<span class="mono">${c.id}</span>`)}${fact("引擎", '<span class="mono">unix:///var/run/docker.sock</span>')}</dl></div>`
    : "";
  if (type === "confirm-remove" || type === "confirm-kill") {
    title = `${type === "confirm-remove" ? "删除" : "强制停止"} ${c.name}？`;
    dangerous = true;
    body = `<p>${type === "confirm-remove" ? "容器及其可写层将永久删除。镜像与挂载的卷会保留。" : "容器内的进程将立即终止，可能来不及完成清理或保存数据。"}</p>${targetSummary}`;
    if (type === "confirm-remove" && c.state !== "exited") {
      body +=
        '<div class="notice">容器仍在运行。请先停止容器，再执行删除。</div>';
      primary = "";
    } else primary = type === "confirm-remove" ? "删除容器" : "强制停止";
  } else if (type === "pull") {
    title = "拉取镜像";
    primary = "开始拉取";
    body = `${field("镜像名称", input("image", data.image || "", 'required placeholder="例如 nginx:1.27-alpine" autocomplete="off"'), "支持仓库地址、Tag 或 Digest。")}${disclosure("高级选项", "平台", field("平台", '<select name="platform"><option>引擎默认平台</option><option>linux/amd64</option><option>linux/arm64</option></select>'))}<p class="small muted" style="margin-top:12px">使用远程服务器已有的镜像仓库凭据。</p>`;
  } else if (type === "create") {
    title = "创建容器";
    primary = "创建并启动";
    wide = true;
    body = `<div class="form-columns">${field("镜像", `<select name="image">${images.map((i) => `<option ${i.name === data.image ? "selected" : ""}>${escapeHtml(i.name)}</option>`).join("")}</select>`)}${field("容器名称", input("name", "", 'required placeholder="例如 preview-web" pattern="[a-zA-Z0-9][a-zA-Z0-9_.-]*"'))}</div>${field("重启策略", '<select name="policy"><option>unless-stopped</option><option>no</option><option>on-failure</option><option>always</option></select>')}${disclosure("端口发布", "可选", `<div class="form-columns">${field("宿主端口", input("hostPort", "8081", 'type="number" min="1" max="65535"'))}${field("容器端口", input("containerPort", "80", 'type="number" min="1" max="65535"'))}</div>${field("绑定地址", input("bind", "127.0.0.1"), "仅本机访问；需要对外提供服务时再修改。")}`, true)}${disclosure("存储", "可选", field("宿主机路径", input("mount", "", 'placeholder="/srv/preview/config"')) + field("容器路径", input("destination", "", 'placeholder="/etc/nginx/conf.d"')) + '<label class="check-row"><input type="checkbox" name="readOnly" checked />只读挂载</label>')}${disclosure("环境变量与网络", "高级", field("环境变量", input("env", "", 'placeholder="APP_ENV=production"')) + field("网络", '<select name="network"><option>bridge</option><option>storefront_default</option></select>'))}<p class="small muted" style="margin-top:12px">配置将在当前引擎创建一个新的独立容器。</p>`;
  } else if (type === "rename") {
    title = "重命名容器";
    primary = "保存名称";
    body = field(
      "容器名称",
      input("name", c.name, 'required pattern="[a-zA-Z0-9][a-zA-Z0-9_.-]*"'),
    );
  } else if (type === "policy") {
    title = "重启策略";
    primary = "应用策略";
    body = `${field("策略", '<select name="policy"><option>unless-stopped</option><option>no</option><option>on-failure</option><option>always</option></select>')}<div class="notice neutral">此容器属于 storefront。请同时更新 Compose 配置，以免下次重建覆盖此设置。</div>`;
  } else if (type === "tag") {
    title = "添加镜像 Tag";
    primary = "添加 Tag";
    body = `<p class="mono">${escapeHtml(data.image)}</p>${field("新引用", input("tag", "", 'required placeholder="例如 nginx:production"'))}`;
  } else if (type === "remove-image") {
    const image = iByName(data.image),
      tags = images.filter((i) => i.id === image.id);
    title = tags.length > 1 ? "移除镜像引用？" : "删除镜像？";
    dangerous = true;
    body = `<p class="mono">${escapeHtml(image.name)}</p><p>${tags.length > 1 ? "仅移除此 Tag。镜像仍有其他引用，镜像数据会保留。" : "删除当前引擎中的镜像，不会删除镜像仓库中的内容。"}</p>${image.used.length && tags.length === 1 ? `<div class="notice">仍被 ${image.used.map(escapeHtml).join("、")} 引用。请先处理相关容器。</div>` : ""}`;
    primary =
      image.used.length && tags.length === 1
        ? ""
        : tags.length > 1
          ? "移除此 Tag"
          : "删除镜像";
  } else if (type === "associate") {
    title = "关联 Compose 项目";
    primary = "关联项目";
    body = `${field("项目名称", input("project", "storefront", "required"))}${field("配置文件", input("path", "/srv/storefront/compose.yaml", "required"), "填写当前 SSH 服务器上的绝对路径。")}${disclosure("高级选项", "环境与 Profile", field("环境文件", input("envfile", "/srv/storefront/.env")) + field("Profiles", input("profiles", "", 'placeholder="默认"')))}`;
  } else if (type === "update") {
    title = "更新 storefront";
    primary = "拉取并更新";
    wide = true;
    body =
      '<p>拉取镜像并应用当前 Compose 配置。有变更的服务会重新创建，期间可能短暂不可用。</p><div class="object-summary"><dl class="facts">' +
      fact("项目", "storefront") +
      fact("配置", '<span class="mono">/srv/storefront/compose.yaml</span>') +
      fact("服务", "api-gateway、order-service、postgres、redis") +
      '</dl></div><label class="check-row"><input type="checkbox" name="pull" checked />更新前拉取镜像</label><p class="small muted">挂载的卷保留。完成后核对服务运行与健康状态。</p>';
  } else if (type === "down") {
    title = "停止并移除 storefront？";
    primary = "停止并移除";
    dangerous = true;
    body =
      '<p>将停止并移除 4 个服务容器，以及项目网络。</p><div class="object-summary"><dl class="facts">' +
      fact("容器", "api-gateway、order-service、postgres、redis") +
      fact("网络", "storefront_default") +
      fact("保留", "命名卷、匿名卷、镜像与项目配置") +
      '</dl></div><p class="small muted">已关联的配置仍可用于重新部署。匿名卷不会保证在下次部署时自动重新挂载。</p>';
  } else if (type === "cleanup") {
    title = "选择要清理的内容";
    primary = "预览清理范围";
    wide = true;
    body =
      '<p>按类型选择候选项，下一步核对具体对象。</p><label class="check-row"><input type="checkbox" name="containers" checked />已停止容器 · 1 个</label><label class="check-row"><input type="checkbox" name="images" checked />未被引用的旧镜像 · 1 个</label><label class="check-row"><input type="checkbox" name="cache" />构建缓存 · 8 项</label><div class="notice neutral" style="margin-top:16px">本次清理不包含卷，服务数据不会随本次选择删除。</div>';
  } else if (type === "cleanup-confirm") {
    title = "清理所选内容？";
    primary = "清理所选内容";
    dangerous = true;
    body = `<p>以下对象将被删除，此操作无法撤销。</p><div class="object-summary">${data.containers ? '<div class="info-row"><div>nightly-backup<p>已停止容器 · 可写层将删除</p></div></div>' : ""}${data.images ? '<div class="info-row"><div>company/orders:2.3<p>未被容器引用的镜像</p></div></div>' : ""}${data.cache ? '<div class="info-row"><div>构建缓存 · 8 项<p>实际释放量在完成后显示</p></div></div>' : ""}</div><p class="small muted">执行前再次核对引用。候选变化时先更新预览。</p>`;
  } else if (type === "forward") {
    title = "新建端口转发";
    subtitle = "Production · api-gateway";
    primary = "创建转发";
    body = `${field("名称", input("name", "api-gateway HTTP", "required"))}<div class="form-columns">${field("本地地址", input("local", "127.0.0.1", "required"))}${field("本地端口", input("port", "8080", 'required type="number" min="1" max="65535"'))}</div><div class="object-summary"><dl class="facts">${fact("经由", "Production")}${fact("远端目标", '<span class="mono">127.0.0.1:8080</span>')}</dl></div><p class="small muted">通过 SSH 访问宿主机已发布的端口。</p>`;
  } else if (type === "volume" || type === "network-detail") {
    const volume = type === "volume";
    title = volume ? "storefront_data" : "storefront_default";
    body = `<dl class="facts">${fact("类型", volume ? "local · 命名卷" : "bridge")}${fact("使用中", volume ? "postgres" : "4 个容器")}${fact(volume ? "容量" : "子网", volume ? "4.8 GB" : "172.20.0.0/16")}${fact("所属项目", "storefront")}</dl><div class="notice neutral" style="margin-top:16px">此资源仍被容器使用，暂不能删除。</div>`;
  } else if (type === "new-volume" || type === "new-network") {
    title = type === "new-volume" ? "创建卷" : "创建网络";
    primary = "创建";
    body =
      field(
        "名称",
        input("name", "", 'required placeholder="例如 storefront_backup"'),
      ) +
      field(
        "驱动",
        `<select><option>${type === "new-volume" ? "local" : "bridge"}</option></select>`,
      );
  } else if (type === "project-logs") {
    title = "项目日志";
    subtitle = "storefront · 选择服务";
    body = containers
      .filter((c) => c.project)
      .map(
        (c) =>
          `<div class="info-row"><div>${c.name}</div>${btn("查看日志", "logs", `class="link" data-id="${c.id}"`)}</div>`,
      )
      .join("");
  } else if (type === "logs" || type === "terminal") {
    title = `${c.name} · ${type === "logs" ? "日志" : "终端"}`;
    subtitle = "沿用现有独立窗口 · 内容预览";
    wide = true;
    body = `<pre class="code-preview">${type === "logs" ? "[09:41:08] INFO  Application started\n[09:41:09] INFO  Listening on :80\n[09:42:15] INFO  GET /health 200\n[09:42:18] INFO  GET /api/orders 200" : "/ # pwd\n/\n/ # "}</pre>`;
  } else if (type === "inspect" || type === "compose-file") {
    title = type === "inspect" ? "容器配置" : "compose.yaml";
    subtitle =
      type === "inspect" ? c.name : "/srv/storefront · 沿用现有文件编辑器";
    wide = true;
    body = `<pre class="code-preview">${escapeHtml(type === "inspect" ? JSON.stringify({ Name: c.name, Image: c.image, State: { Status: c.state, Health: c.health }, HostConfig: { RestartPolicy: "unless-stopped" }, Env: "值已隐藏" }, null, 2) : 'services:\n  api-gateway:\n    image: nginx:1.27-alpine\n    ports:\n      - "8080:80"\n    volumes:\n      - ./config:/etc/nginx/conf.d:ro\n    restart: unless-stopped')}</pre>`;
  } else if (type === "progress") {
    title = data.label;
    subtitle = "后台任务 · 可以继续浏览其他对象";
    body =
      '<div class="step-list"><div class="running">' +
      icon("check") +
      '已连接镜像仓库</div><div><i class="spinner"></i>正在下载镜像层<small>3 / 6 层完成</small></div><div class="muted">' +
      icon("layers") +
      '校验与解压</div></div><div class="progress-track indeterminate"><span></span></div><p class="small muted">关闭此窗口后任务继续运行。</p>';
  } else if (type === "task") {
    title = data.label;
    subtitle = {
      running: "正在执行",
      success: "操作已完成",
      failed: "操作未完成",
      unknown: "结果待确认",
    }[data.state];
    body = `<div class="notice ${data.state === "failed" ? "error" : data.state === "unknown" ? "" : "neutral"}">${{ running: "操作正在远端执行，可返回工作区继续使用。", success: "已核对执行结果，相关对象列表已刷新。", failed: "当前账号没有执行此操作的权限。容器与原有配置未被自动清理。", unknown: "连接在执行中断开，远端可能已经完成操作。请先核对当前状态。" }[data.state]}</div>${data.state === "unknown" ? btn("核对当前状态", "reconcile", `data-task="${data.id}" class="primary"`) : ""}`;
  } else if (type === "all-tasks") {
    title = "操作记录";
    subtitle = "当前应用会话";
    body = tasks.length
      ? tasks
          .map(
            (t) =>
              `<div class="info-row"><div><strong>${t.label}</strong><p>${{ running: "执行中", success: "已完成", failed: "未完成", unknown: "结果待确认" }[t.state]}</p></div>${btn("详情", "task", `class="link" data-task="${t.id}"`)}</div>`,
          )
          .join("")
      : '<p class="muted">暂无管理操作。</p>';
  }
  const dialog = $("#dialog");
  dialog.classList.toggle("wide", wide);
  dialog.innerHTML = `<form id="dialog-form"><header class="dialog-heading"><div><h2 id="dialog-title">${escapeHtml(title)}</h2><p>${escapeHtml(subtitle)}</p></div>${btn(icon("close"), "close-dialog", 'class="icon" type="button" aria-label="关闭对话框"')}</header><div class="dialog-body">${body}</div><footer class="dialog-actions">${btn(type === "progress" ? "后台运行" : primary ? "取消" : "关闭", "close-dialog", `type="button" ${dangerous ? "autofocus" : ""}`)}${primary ? `<button type="submit" class="${dangerous ? "danger-fill" : "primary"}">${primary}</button>` : ""}</footer></form>`;
  // Buttons inside form bodies are navigation actions, never implicit submits.
  dialog
    .querySelectorAll("button[data-action]")
    .forEach((button) => (button.type = "button"));
  if (!dialog.open) dialog.showModal();
  const first = dangerous
    ? dialog.querySelector(".dialog-actions [data-action=close-dialog]")
    : dialog.querySelector("input,select") ||
      dialog.querySelector(".dialog-actions button");
  first?.focus();
  $("#dialog-form").addEventListener("submit", submitDialog);
}
function submitDialog(event) {
  event.preventDefault();
  const form = new FormData(event.target),
    data = Object.fromEntries(form),
    type = modalType,
    context = modalData;
  if (type === "confirm-remove" || type === "confirm-kill") {
    const id = context.id || view.id;
    closeDialog();
    containerAction(type === "confirm-remove" ? "remove" : "kill", id);
  } else if (type === "pull") {
    const value = data.image.trim();
    if (!value || /\s/.test(value)) {
      event.target.elements.image.setCustomValidity(
        "请输入不含空格的镜像引用。",
      );
      event.target.elements.image.reportValidity();
      return;
    }
    const task = runTask(
      `拉取 ${value}`,
      `image:${value}`,
      () => {
        if (!iByName(value))
          images.push({
            id: "sha256:6a12b8f98d31",
            name: value,
            size: "48.2 MB",
            used: [],
          });
        view.page = "images";
        view.id = value;
      },
      "pull",
    );
    if (task) showDialog("progress", task);
  } else if (type === "create") {
    if (containers.some((c) => c.name === data.name)) {
      event.target.elements.name.setCustomValidity("这个容器名称已经存在。");
      event.target.elements.name.reportValidity();
      return;
    }
    runTask(`创建 ${data.name}`, "create:" + data.name, () => {
      const c = {
        ...clone(initialContainers[0]),
        id: `abc${String(serial).padStart(9, "0")}`,
        name: data.name,
        image: data.image,
        project: "",
        health: "none",
        age: "刚刚",
      };
      containers.push(c);
      view.page = "containers";
      view.id = c.id;
    });
  } else if (type === "rename") {
    const c = cById(context.id || view.id);
    runTask(`重命名 ${c.name}`, c.id, () => {
      c.name = data.name;
    });
  } else if (type === "policy") {
    const c = cById(context.id || view.id);
    runTask(`更新 ${c.name} 的重启策略`, c.id, () => {
      c.policy = data.policy;
    });
  } else if (type === "tag") {
    const image = iByName(context.image);
    runTask(`添加 ${data.tag}`, "image:" + image.id, () =>
      images.push({ ...clone(image), name: data.tag }),
    );
  } else if (type === "remove-image")
    runTask(`移除 ${context.image}`, "image:" + context.image, () => {
      images = images.filter((i) => i.name !== context.image);
      view.id = null;
    });
  else if (type === "update")
    runTask("更新 storefront", "project:storefront", () => {
      containers
        .filter((c) => c.project)
        .forEach((c) => {
          c.health = "healthy";
          c.state = "running";
          c.age = "刚刚";
        });
    });
  else if (type === "down")
    runTask("移除 storefront", "project:storefront", () => {
      containers = containers.filter((c) => !c.project);
      view.page = "projects";
      view.id = null;
    });
  else if (type === "cleanup") {
    if (!data.containers && !data.images && !data.cache) {
      announce("请至少选择一类清理内容。");
      return;
    }
    showDialog("cleanup-confirm", data);
  } else if (type === "cleanup-confirm")
    runTask("清理所选资源", "cleanup", () => {
      if (context.containers)
        containers = containers.filter((c) => c.state !== "exited");
      if (context.images) images = images.filter((i) => i.used.length);
    });
  else if (type === "associate")
    runTask(`关联 ${data.project}`, "project:" + data.project, () => {
      view.page = "projects";
      view.id = "storefront";
    });
  else if (type === "forward")
    runTask("创建 api-gateway 端口转发", "forward", () => {});
  else if (type === "new-volume" || type === "new-network")
    runTask(
      `创建${type === "new-volume" ? "卷" : "网络"} ${data.name}`,
      type + data.name,
      () => {},
    );
}
document.addEventListener("click", async (event) => {
  const b = event.target.closest("[data-action]");
  if (!b || b.disabled) {
    if (!event.target.closest("#menu")) closeMenu(false);
    return;
  }
  const { action, id, kind, page, image: imageName, task: taskId } = b.dataset;
  if (action !== "menu") closeMenu(false);
  if (["start", "stop", "restart", "pause", "unpause"].includes(action)) {
    containerAction(action, id);
    return;
  }
  if (action === "menu") {
    if (!$("#menu").hidden && menuTrigger === b) closeMenu();
    else showMenu(b);
  } else if (action === "select") navigate(kind, id);
  else if (action === "page") {
    view.query = "";
    view.filter = "all";
    navigate(page);
  } else if (action === "back") {
    const oldId = view.id;
    navigate(view.page);
    $("#page")
      .querySelector(`[data-action=select][data-id="${CSS.escape(oldId)}"]`)
      ?.focus({ preventScroll: true });
  } else if (action === "resources") navigate("resources");
  else if (action === "resources-back") navigate("containers");
  else if (action === "clear-filter") {
    view.query = "";
    view.filter = "all";
    render();
    $("#object-search")?.focus();
  } else if (action === "monitor" || action === "docker") {
    view.service = action;
    render();
  } else if (action === "refresh") {
    if (scenario === "offline") {
      scenario = "normal";
      $("#scenario").value = "normal";
    }
    render();
    announce("Docker 状态已刷新。");
  } else if (action === "project-detail") navigate("projects", "storefront");
  else if (action === "project-stop" || action === "project-restart")
    runTask(
      `${action === "project-stop" ? "停止" : "重启"} storefront`,
      "project:storefront",
      () =>
        containers
          .filter((c) => c.project)
          .forEach((c) => {
            c.state = action === "project-stop" ? "exited" : "running";
          }),
    );
  else if (action === "close-dialog") closeDialog();
  else if (action === "copy") {
    try {
      await navigator.clipboard.writeText(b.dataset.copy);
      b.innerHTML = icon("check");
      b.setAttribute("aria-label", "已复制");
      announce("已复制。");
    } catch {
      announce("浏览器未允许复制，请选择文字复制。");
    }
  } else if (action === "reveal-env") {
    const revealed = b.getAttribute("aria-pressed") === "true";
    $("#env-value").textContent = revealed ? "••••••" : "production";
    b.setAttribute("aria-pressed", String(!revealed));
    b.setAttribute("aria-label", `${revealed ? "显示" : "隐藏"} APP_ENV 的值`);
  } else if (action === "locate") {
    $(".file-location .field").textContent = "/srv/storefront/config";
    announce("已在文件面板定位 /srv/storefront/config。");
  } else if (action === "task") {
    const task = tasks.find((t) => t.id === Number(taskId));
    if (task) showDialog("task", task);
  } else if (action === "reconcile") {
    const task = tasks.find((t) => t.id === Number(taskId));
    if (task) {
      task.state = "success";
      task.mutate?.();
      closeDialog();
      render();
      announce("已核对当前状态。");
    }
  } else showDialog(action, { id, image: imageName });
});
document.addEventListener("input", (event) => {
  if (event.target.id === "object-search") {
    const pos = event.target.selectionStart;
    view.query = event.target.value;
    renderList();
    $("#object-search").focus();
    $("#object-search").setSelectionRange(pos, pos);
  }
  if (event.target.matches("#dialog input")) event.target.setCustomValidity("");
});
document.addEventListener("change", (event) => {
  if (event.target.id === "state-filter") {
    view.filter = event.target.value;
    renderList();
    $("#state-filter").focus();
  }
});
document.addEventListener("keydown", (event) => {
  if (!$("#menu").hidden) {
    const entries = [...$("#menu").querySelectorAll("button:not(:disabled)")],
      index = entries.indexOf(document.activeElement);
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      const next =
        event.key === "Home"
          ? 0
          : event.key === "End"
            ? entries.length - 1
            : (index + (event.key === "ArrowDown" ? 1 : -1) + entries.length) %
              entries.length;
      entries[next]?.focus();
    }
    if (event.key === "Escape") {
      event.preventDefault();
      closeMenu();
    }
  } else if (
    event.target.matches(".row-main") &&
    ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)
  ) {
    const entries = [...$("#page").querySelectorAll(".row-main")],
      index = entries.indexOf(event.target);
    event.preventDefault();
    entries[
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? entries.length - 1
          : Math.max(
              0,
              Math.min(
                entries.length - 1,
                index + (event.key === "ArrowDown" ? 1 : -1),
              ),
            )
    ]?.focus();
  } else if (
    event.target.matches(".row-main") &&
    event.shiftKey &&
    event.key === "F10"
  ) {
    event.preventDefault();
    showMenu(
      event.target.parentElement.querySelector(".row-menu"),
      event.target,
    );
  }
});
$("#dialog").addEventListener("close", () => {
  if ($("#dialog").open) return;
  modalType = null;
  modalData = null;
  const active = document.activeElement;
  if (
    dialogTrigger?.isConnected &&
    (active === document.body || $("#dialog").contains(active))
  )
    dialogTrigger.focus({ preventScroll: true });
});
$("#dialog").addEventListener("click", (event) => {
  if (event.target === $("#dialog")) {
    const rect = $("#dialog").getBoundingClientRect();
    if (
      event.clientX < rect.left ||
      event.clientX > rect.right ||
      event.clientY < rect.top ||
      event.clientY > rect.bottom
    )
      closeDialog();
  }
});
$("#theme").addEventListener("click", () => {
  const dark = document.documentElement.dataset.theme === "dark";
  document.documentElement.dataset.theme = dark ? "light" : "dark";
  $("#theme").textContent = dark ? "深色" : "浅色";
  $("#theme").setAttribute("aria-label", `切换${dark ? "深色" : "浅色"}外观`);
});
$("#panel-width").addEventListener("change", (event) =>
  $(".app-shell").style.setProperty(
    "--service-width",
    `${event.target.value}px`,
  ),
);
$("#scenario").addEventListener("change", (event) => {
  scenario = event.target.value;
  closeDialog();
  closeMenu(false);
  if (["empty", "offline"].includes(scenario)) view.id = null;
  render();
});
$("#reset").addEventListener("click", () => {
  resetGeneration++;
  closeDialog();
  closeMenu(false);
  containers = clone(initialContainers);
  images = clone(initialImages);
  tasks = [];
  view = {
    page: "containers",
    id: null,
    query: "",
    filter: "all",
    service: "docker",
  };
  scenario = "normal";
  $("#scenario").value = "normal";
  render();
});
render();
