export function routeRecords(target, servers, proxies) {
  return (target.route || []).map(step => {
    const record = (step.kind === 'ssh' ? servers : proxies).find(record => record.id === (step.kind === 'ssh' ? step.serverId : step.proxyId));
    if (!record) throw new Error('连接路径引用的资料不存在');
    return { kind: step.kind, record };
  });
}

// Each target contributes its explicit route; referenced records never expand their own routes.
export function buildConnectionTree(targets, servers, proxies) {
  const root = { key: 'local', children: [] }, nodes = new Map([['local', root]]);
  for (const target of targets) {
    const steps = [...routeRecords(target, servers, proxies), { kind: target.protocol || 'ssh', record: target }];
    let parent = root;
    for (const [index, step] of steps.entries()) {
      const key = parent.key + '/' + step.kind + ':' + encodeURIComponent(step.record.id);
      let node = nodes.get(key);
      if (!node) {
        node = { ...step, key, parentKey: parent.key, children: [], target: false };
        parent.children.push(node); nodes.set(key, node);
      }
      if (index === steps.length - 1) node.target = true;
      parent = node;
    }
  }
  return root;
}

export function filterConnectionTree(root, matches) {
  const visit = node => {
    const children = node.children.map(visit).filter(Boolean);
    const matched = !!node.target && matches(node.record);
    return node.key === 'local' || matched || children.length ? { ...node, matched, children } : null;
  };
  return visit(root);
}

export function visibleConnectionNodes(root, collapsed = new Set(), filtering = false) {
  const rows = [];
  const visit = (parent, depth) => parent.children.forEach((node, index) => {
    const expanded = !!node.children.length && (filtering || !collapsed.has(node.key));
    rows.push({ ...node, depth, expanded, position: index + 1, siblings: parent.children.length });
    if (expanded) visit(node, depth + 1);
  });
  visit(root, 1);
  return rows;
}
