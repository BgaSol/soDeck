const containerCollections = ['ports', 'mounts', 'networks'];
const sameResource = (a, b) => a?.kind === b?.kind && a?.id === b?.id;

// One observation and one cached page per collection, owned by this mounted object.
export class ResourceDetailsData {
  constructor(request, changed) {
    this.request = request;
    this.changed = changed;
    this.epoch = 0;
    this.pages = new Map();
    this.errors = new Map();
  }

  mount(node, snapshot, navigation = {}) {
    this.dispose();
    this.node = node;
    this.snapshot = snapshot;
    this.observation = null;
    this.error = '';
    this.tab = ['overview', ...containerCollections].includes(navigation.tab) ? navigation.tab : 'overview';
    this.offsets = { ...navigation.offsets };
    this.previousOffsets = Object.fromEntries(Object.entries(navigation.previousOffsets || {}).map(([name, offsets]) => [name, [...offsets]]));
    this.pages.clear();
    this.errors.clear();
  }

  update(node, snapshot) { this.node = node; this.snapshot = snapshot; }
  enabled(value) { this.active = value; if (!value) this.wanted = false; }
  offset(name) { return this.offsets[name] || 0; }
  previous(name) { return this.previousOffsets[name]?.at(-1) ?? (this.offset(name) ? 0 : null); }
  page(name) {
    const page = this.pages.get(name);
    return page && page.observationRevision === this.observation?.observationRevision && page.offset === this.offset(name) ? page : null;
  }
  navigation() {
    return this.node?.resource.kind === 'container' ? { tab: this.tab, offsets: { ...this.offsets }, previousOffsets: structuredClone(this.previousOffsets) } : {};
  }
  select(tab) {
    if (!['overview', ...containerCollections].includes(tab) || tab === this.tab) return false;
    this.tab = tab;
    return true;
  }
  navigate(name, direction) {
    const page = this.page(name);
    if (!page) return false;
    const history = this.previousOffsets[name] ||= [];
    if (direction === 'next') {
      if (!Number.isSafeInteger(page.nextOffset) || page.nextOffset <= this.offset(name) || page.nextOffset >= page.total) return false;
      history.push(this.offset(name));
      this.offsets[name] = page.nextOffset;
    } else {
      if (!this.offset(name)) return false;
      this.offsets[name] = history.pop() ?? 0;
    }
    this.errors.delete(name);
    return true;
  }
  collections() {
    if (this.node.resource.kind === 'container') return containerCollections.includes(this.tab) ? [this.tab] : [];
    return [...Object.keys(this.observation?.info?.counts || {}), 'references'];
  }
  valid(epoch, scope, resource) {
    return this.active && epoch === this.epoch && scope === this.snapshot?.scopeEpoch && sameResource(resource, this.node?.resource);
  }
  refresh() {
    if (!this.active || !this.node) return Promise.resolve();
    this.wanted = true;
    if (this.running) return this.running;
    const epoch = this.epoch;
    const run = this.read(epoch).finally(() => { if (this.running === run) this.running = null; });
    this.running = run;
    return run;
  }
  async read(epoch) {
    while (this.wanted && this.active && epoch === this.epoch) {
      this.wanted = false;
      const resource = { ...this.node.resource }, scope = this.snapshot.scopeEpoch;
      try {
        const observation = await this.request({ action: 'browser.object', resource, scopeEpoch: scope });
        if (!this.valid(epoch, scope, resource) || !observation || observation.scopeEpoch !== scope || !sameResource(observation.resource, resource)) continue;
        const revision = observation.observationRevision;
        if (this.observation?.observationRevision != null && revision != null && BigInt(revision) < BigInt(this.observation.observationRevision)) continue;
        if (revision !== this.observation?.observationRevision) { this.pages.clear(); this.errors.clear(); }
        if (observation.relation?.count !== this.observation?.relation?.count) this.pages.delete('references');
        this.observation = observation;
        this.error = '';
        for (const [name, count] of Object.entries({ ...observation.info?.counts, references: observation.relation?.count })) {
          if (Number.isSafeInteger(count) && this.offset(name) > 0 && this.offset(name) >= count) {
            this.offsets[name] = Math.max(0, count - 1);
            this.previousOffsets[name] = (this.previousOffsets[name] || []).filter(offset => offset < this.offset(name));
          }
        }
        this.changed();
        if (!observation.info || !revision) continue;
        for (const name of this.collections()) {
          if (!this.valid(epoch, scope, resource)) break;
          if (resource.kind === 'container' && name !== this.tab) break;
          if (this.page(name) && !this.errors.has(name)) continue;
          const offset = this.offset(name);
          try {
            const page = await this.request({ action: 'browser.collection', resource, scopeEpoch: scope, observationRevision: revision, collection: name, offset });
            if (!this.valid(epoch, scope, resource) || !page || page.scopeEpoch !== scope || !sameResource(page.resource, resource) || page.observationRevision !== revision || this.observation.observationRevision !== revision || page.offset !== offset) continue;
            this.pages.set(name, page);
            this.errors.delete(name);
          } catch (error) {
            if (this.valid(epoch, scope, resource) && this.offset(name) === offset) this.errors.set(name, error.message || String(error));
          }
          if (this.valid(epoch, scope, resource)) this.changed();
        }
      } catch (error) {
        if (this.valid(epoch, scope, resource)) { this.error = error.message || String(error); this.changed(); }
      }
    }
  }
  dispose() { this.epoch++; this.active = false; this.wanted = false; this.running = null; this.node = null; this.pages.clear(); this.errors.clear(); }
}
