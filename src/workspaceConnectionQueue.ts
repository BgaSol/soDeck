/** One connection setup per server workspace; established terminals run freely. */
export class WorkspaceConnectionQueue {
  private waiting: {
    signal: AbortSignal;
    resolve: (release: () => void) => void;
  }[] = [];
  private busy = false;
  acquire(signal: AbortSignal): Promise<() => void> {
    return new Promise((resolve) => {
      const item = { signal, resolve };
      this.waiting.push(item);
      signal.addEventListener(
        "abort",
        () => {
          this.waiting = this.waiting.filter((w) => w !== item);
        },
        { once: true },
      );
      this.drain();
    });
  }
  private drain() {
    if (this.busy) return;
    const item = this.waiting.shift();
    if (!item) return;
    if (item.signal.aborted) {
      this.drain();
      return;
    }
    this.busy = true;
    let released = false;
    const release = () => {
      if (released) return;
      released = true;
      this.busy = false;
      this.drain();
    };
    item.signal.addEventListener("abort", release, { once: true });
    item.resolve(release);
  }
}
