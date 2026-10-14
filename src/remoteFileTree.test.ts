import { describe, expect, it, vi } from "vitest";
import {
  RemoteFileTree,
  directoryEntry,
  visibleFileRows,
} from "./remoteFileTree";
import type { Directory, DirectoryBatch } from "./types";

function deferred<T>() {
  let resolve!: (result: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const directory = (path: string, children: string[] = []): Directory => ({
  path,
  resolvedPath: path,
  entries: children.map(directoryEntry),
});
const batch = (
  path: string,
  paths: string[],
  homePath = "/home/ops",
): DirectoryBatch => ({
  homePath,
  path,
  directories: paths.map((item) => ({
    path: item,
    directory: directory(item),
  })),
});
const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

describe("remote file tree", () => {
  it("loads only the home path, lazily loads a branch once, and keeps it cached after collapse", async () => {
    const api = {
      directoryChain: vi.fn(async () => ({
        homePath: "/home/ops",
        path: "/home/ops",
        directories: [
          { path: "/", directory: directory("/", ["/etc", "/home"]) },
          { path: "/home", directory: directory("/home", ["/home/ops"]) },
          {
            path: "/home/ops",
            directory: directory("/home/ops", ["/home/ops/projects"]),
          },
        ],
      })),
      directory: vi.fn(async (session: string, path = "/") => {
        expect(session).toBe("one");
        return directory(path, [path + "/child"]);
      }),
    };
    const model = new RemoteFileTree("one", api);
    model.activate();
    await tick();
    expect(model.getState().expanded).toEqual(["/", "/home", "/home/ops"]);
    expect(model.getState().selected).toBe("/home/ops");
    expect(api.directory).not.toHaveBeenCalled();
    expect(
      visibleFileRows(model.getState()).map((row) => row.file.path),
    ).toEqual(["/", "/etc", "/home", "/home/ops", "/home/ops/projects"]);
    await model.expand("/etc");
    model.collapse("/etc");
    await model.expand("/etc");
    expect(api.directory).toHaveBeenCalledTimes(1);
    expect(model.getState().expanded).toContain("/home/ops");
  });

  it("lets only the newest navigation select and reveal while retaining older cached results", async () => {
    const first = deferred<DirectoryBatch>();
    const second = deferred<DirectoryBatch>();
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn((_session: string, path?: string) =>
        path === "/a" ? first.promise : second.promise,
      ),
    };
    const model = new RemoteFileTree("one", api, {
      sessionId: "one",
      directories: { "/": { directory: directory("/", ["/a", "/b"]) } },
      expanded: ["/"],
      selected: "",
      currentPath: "/",
      homePath: "/",
      scrollTop: 0,
    });
    const a = model.revealPath("/a");
    await tick();
    const b = model.revealPath("/b");
    await tick();
    second.resolve(batch("/b", ["/b"]));
    await b;
    first.resolve(batch("/a", ["/a"]));
    await a;
    expect(model.getState().selected).toBe("/b");
    expect(model.getState().currentPath).toBe("/b");
    expect(model.getState().expanded).not.toContain("/a");
    expect(model.getState().directories["/a"].directory?.path).toBe("/a");
  });

  it("does not let late home initialization replace a terminal directory change", async () => {
    const home = deferred<DirectoryBatch>();
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn((_session: string, path?: string) =>
        path ? Promise.resolve(batch(path, [path])) : home.promise,
      ),
    };
    const model = new RemoteFileTree("one", api);
    model.activate();
    await tick();
    model.acceptTerminalDirectory(
      { sessionId: "one", path: "/var", sequence: 2 },
      true,
    );
    home.resolve(batch("/home/ops", ["/", "/home", "/home/ops"]));
    await tick();
    expect(model.getState().selected).toBe("/var");
    expect(model.getState().expanded).toEqual(["/", "/var"]);
    expect(api.directoryChain.mock.calls[1][1]).toBe("/var");
  });

  it("deduplicates an expansion while reveal is already loading its ancestor", async () => {
    const response = deferred<DirectoryBatch>();
    const api = {
      directory: vi.fn(async () => directory("/a")),
      directoryChain: vi.fn(() => response.promise),
    };
    const model = new RemoteFileTree("one", api);
    const reveal = model.revealPath("/a/b");
    await tick();
    const expand = model.expand("/a");
    response.resolve(batch("/a/b", ["/", "/a", "/a/b"]));
    await Promise.all([reveal, expand]);
    expect(api.directory).not.toHaveBeenCalled();
  });

  it("shows a traversable target even if its ancestor cannot be listed and allows a local retry", async () => {
    const api = {
      directory: vi.fn(async () => directory("/home", ["/home/ops"])),
      directoryChain: vi.fn(async (): Promise<DirectoryBatch> => ({
        homePath: "/home/ops",
        path: "/home/ops",
        directories: [
          { path: "/", directory: directory("/", ["/home"]) },
          { path: "/home", error: "Permission denied" },
          {
            path: "/home/ops",
            directory: directory("/home/ops", ["/home/ops/project"]),
          },
        ],
      })),
    };
    const model = new RemoteFileTree("one", api);
    await model.revealPath();
    expect(
      visibleFileRows(model.getState()).map((row) => row.file.path),
    ).toEqual(["/", "/home", "/home/ops", "/home/ops/project"]);
    expect(model.getState().directories["/home"].error).toBe(
      "Permission denied",
    );
    await model.refresh("/home");
    expect(model.getState().directories["/home"].error).toBeUndefined();
    expect(model.getState().selected).toBe("/home/ops");
  });

  it("retains logical symlink paths so children remain under the selected alias", async () => {
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn(async (): Promise<DirectoryBatch> => ({
        homePath: "/",
        path: "/bin",
        directories: [
          {
            path: "/",
            directory: {
              ...directory("/", ["/bin"]),
              entries: [{ ...directoryEntry("/bin"), isSymlink: true }],
            },
          },
          {
            path: "/bin",
            directory: {
              ...directory("/bin", ["/bin/tools"]),
              resolvedPath: "/usr/bin",
            },
          },
        ],
      })),
    };
    const model = new RemoteFileTree("one", api);
    await model.revealPath("/bin");
    expect(
      visibleFileRows(model.getState()).map((row) => row.file.path),
    ).toEqual(["/", "/bin", "/bin/tools"]);
    expect(model.getState().currentPath).toBe("/bin");
  });

  it("restores cached branches and scroll while restarting only unfinished expanded loads", async () => {
    const inflight = deferred<Directory>();
    const api = {
      directory: vi.fn(() => inflight.promise),
      directoryChain: vi.fn(async () => batch("/", ["/"])),
    };
    const model = new RemoteFileTree("one", api, {
      sessionId: "one",
      directories: { "/": { directory: directory("/", ["/a", "/b"]) } },
      expanded: ["/"],
      selected: "/b",
      currentPath: "/",
      homePath: "/",
      scrollTop: 270,
    });
    void model.expand("/a");
    const snapshot = JSON.parse(JSON.stringify(model.snapshot()));
    expect(snapshot.directories["/a"]).toEqual({});
    const restoredApi = {
      directory: vi.fn(async () => directory("/a", ["/a/child"])),
      directoryChain: api.directoryChain,
    };
    const restored = new RemoteFileTree("one", restoredApi, snapshot);
    restored.activate();
    await tick();
    expect(restoredApi.directory).toHaveBeenCalledExactlyOnceWith("one", "/a");
    expect(restoredApi.directoryChain).not.toHaveBeenCalled();
    expect(restored.getState().selected).toBe("/b");
    expect(restored.getState().scrollTop).toBe(270);
    expect(
      restored.getState().directories["/a"].directory?.entries[0].path,
    ).toBe("/a/child");
    inflight.resolve(directory("/a"));
  });

  it("ignores wrong sessions and old cwd events, defers hidden-tab navigation, and leaves manual browsing alone on unchanged prompts", async () => {
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn(async (_session: string, path?: string) =>
        batch(path || "/home/ops", [
          "/",
          ...(path ? [path] : ["/home", "/home/ops"]),
        ]),
      ),
    };
    const model = new RemoteFileTree("one", api);
    model.acceptTerminalDirectory(
      { sessionId: "other", path: "/wrong", sequence: 100 },
      true,
    );
    model.acceptTerminalDirectory(
      { sessionId: "one", path: "/var", sequence: 10 },
      false,
    );
    model.acceptTerminalDirectory(
      { sessionId: "one", path: "/old", sequence: 9 },
      true,
    );
    expect(api.directoryChain).not.toHaveBeenCalled();
    model.activate();
    await tick();
    expect(model.getState().selected).toBe("/var");
    model.select("/", true);
    model.acceptTerminalDirectory(
      { sessionId: "one", path: "/var", sequence: 11 },
      true,
    );
    await tick();
    expect(model.getState().selected).toBe("/");
    expect(api.directoryChain).toHaveBeenCalledTimes(1);
  });

  it("asks the remote filesystem to resolve dot-dot after symlinks instead of using a cached lexical parent", async () => {
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn(async () => batch("/usr", ["/usr"])),
    };
    const model = new RemoteFileTree("one", api, {
      sessionId: "one",
      directories: { "/": { directory: directory("/", ["/bin", "/usr"]) } },
      expanded: ["/"],
      selected: "",
      currentPath: "/",
      homePath: "/",
      scrollTop: 0,
    });
    await model.revealPath("/bin/..");
    expect(api.directoryChain).toHaveBeenCalledExactlyOnceWith(
      "one",
      "/bin/..",
      ["/"],
    );
    expect(model.getState().currentPath).toBe("/usr");
  });

  it("does not reopen a branch the user collapsed while automatic reveal was loading", async () => {
    const response = deferred<DirectoryBatch>();
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn(() => response.promise),
    };
    const model = new RemoteFileTree("one", api);
    const reveal = model.revealPath("/home/ops");
    await tick();
    model.collapse("/");
    response.resolve(batch("/home/ops", ["/", "/home", "/home/ops"]));
    await reveal;
    expect(model.getState().expanded).toEqual([]);
    expect(model.getState().selected).toBe("");
    expect(model.getState().revealing).toBe(false);
    expect(model.getState().directories["/home/ops"].directory).toBeDefined();
  });

  it("prunes deleted cached subtrees on refresh and rejects their late in-flight replies", async () => {
    const late = deferred<Directory>();
    const api = {
      directory: vi.fn((_session: string, path?: string) =>
        path === "/a/late" ? late.promise : Promise.resolve(directory("/")),
      ),
      directoryChain: vi.fn(async () => batch("/", ["/"])),
    };
    const model = new RemoteFileTree("one", api, {
      sessionId: "one",
      directories: {
        "/": { directory: directory("/", ["/a"]) },
        "/a": { directory: directory("/a", ["/a/late"]) },
      },
      expanded: ["/", "/a"],
      selected: "/a/late",
      currentPath: "/a",
      homePath: "/",
      scrollTop: 0,
    });
    const expansion = model.expand("/a/late");
    await model.refresh("/");
    late.resolve(directory("/a/late", ["/a/late/child"]));
    await expansion;
    expect(Object.keys(model.getState().directories)).toEqual(["/"]);
    expect(model.getState().expanded).toEqual(["/"]);
    expect(model.getState().selected).toBe("/");
    expect(model.getState().currentPath).toBe("/");
    expect(
      visibleFileRows(model.getState()).map((row) => row.file.path),
    ).toEqual(["/"]);
  });

  it("prefers the terminal HOME when restoring an otherwise uninitialized tree", async () => {
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn(async (_session: string, path?: string) =>
        batch(path || "/wrong-sftp-default", ["/", "/real-home"]),
      ),
    };
    const model = new RemoteFileTree("one", api, {
      sessionId: "one",
      directories: {},
      expanded: ["/"],
      selected: "",
      currentPath: "/",
      homePath: "",
      scrollTop: 0,
      terminalCwd: {
        sessionId: "one",
        path: "/real-home",
        home: "/real-home",
        sequence: 1,
      },
    });
    model.activate();
    await tick();
    expect(api.directoryChain).toHaveBeenCalledExactlyOnceWith(
      "one",
      "/real-home",
      [],
    );
  });

  it("refreshes a cached parent inside the reveal batch after a directory is created or renamed", async () => {
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn(async (): Promise<DirectoryBatch> => ({
        homePath: "/home/ops",
        path: "/home/ops/new/deep",
        directories: [
          {
            path: "/home/ops",
            directory: directory("/home/ops", [
              "/home/ops/old",
              "/home/ops/new",
            ]),
          },
          {
            path: "/home/ops/new",
            directory: directory("/home/ops/new", ["/home/ops/new/deep"]),
          },
          {
            path: "/home/ops/new/deep",
            directory: directory("/home/ops/new/deep"),
          },
        ],
      })),
    };
    const model = new RemoteFileTree("one", api, {
      sessionId: "one",
      directories: {
        "/": { directory: directory("/", ["/home"]) },
        "/home": { directory: directory("/home", ["/home/ops"]) },
        "/home/ops": { directory: directory("/home/ops", ["/home/ops/old"]) },
      },
      expanded: ["/", "/home", "/home/ops"],
      selected: "/home/ops",
      currentPath: "/home/ops",
      homePath: "/home/ops",
      scrollTop: 0,
    });
    await model.revealPath("/home/ops/new/deep");
    expect(api.directoryChain).toHaveBeenCalledExactlyOnceWith(
      "one",
      "/home/ops/new/deep",
      ["/", "/home"],
    );
    expect(api.directory).not.toHaveBeenCalled();
    expect(
      visibleFileRows(model.getState()).map((row) => row.file.path),
    ).toEqual([
      "/",
      "/home",
      "/home/ops",
      "/home/ops/new",
      "/home/ops/new/deep",
      "/home/ops/old",
    ]);
    expect(model.getState().selected).toBe("/home/ops/new/deep");
  });

  it("refreshes a stale file-to-directory edge even when the target directory is already cached", async () => {
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn(async (): Promise<DirectoryBatch> => ({
        homePath: "/",
        path: "/workspace",
        directories: [{ path: "/", directory: directory("/", ["/workspace"]) }],
      })),
    };
    const model = new RemoteFileTree("one", api, {
      sessionId: "one",
      directories: {
        "/": {
          directory: {
            ...directory("/"),
            entries: [{ ...directoryEntry("/workspace"), isDir: false }],
          },
        },
        "/workspace": {
          directory: directory("/workspace", ["/workspace/child"]),
        },
      },
      expanded: ["/"],
      selected: "",
      currentPath: "/",
      homePath: "/",
      scrollTop: 0,
    });
    await model.revealPath("/workspace");
    expect(api.directoryChain).toHaveBeenCalledExactlyOnceWith(
      "one",
      "/workspace",
      ["/workspace"],
    );
    expect(
      visibleFileRows(model.getState()).map((row) => row.file.path),
    ).toEqual(["/", "/workspace", "/workspace/child"]);
  });

  it("keeps shell HOME over the SFTP default and expands tilde addresses with that HOME", async () => {
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn(
        async (_session: string, path?: string): Promise<DirectoryBatch> => ({
          homePath: "/sftp-default",
          path: path || "/sftp-default",
          directories:
            path === "/work"
              ? [
                  {
                    path: "/",
                    directory: directory("/", ["/work", "/real-home"]),
                  },
                  { path: "/work", directory: directory("/work") },
                ]
              : [
                  {
                    path: "/real-home",
                    directory: directory("/real-home", ["/real-home/projects"]),
                  },
                  {
                    path: "/real-home/projects",
                    directory: directory("/real-home/projects"),
                  },
                ],
        }),
      ),
    };
    const model = new RemoteFileTree("one", api);
    model.acceptTerminalDirectory(
      { sessionId: "one", path: "/work", home: "/real-home", sequence: 1 },
      true,
    );
    await tick();
    expect(model.getState().homePath).toBe("/real-home");
    await model.revealPath("~/projects");
    expect(api.directoryChain.mock.calls[1][1]).toBe("/real-home/projects");
    expect(model.getState().homePath).toBe("/real-home");
    expect(model.getState().currentPath).toBe("/real-home/projects");
    await model.revealPath("~");
    expect(model.getState().currentPath).toBe("/real-home");
    expect(api.directoryChain).toHaveBeenCalledTimes(2);
  });

  it("keeps a manual selection when a previous automatic reveal completes late", async () => {
    const pending = deferred<DirectoryBatch>();
    const api = {
      directory: vi.fn(async () => directory("/")),
      directoryChain: vi.fn(() => pending.promise),
    };
    const model = new RemoteFileTree("one", api, {
      sessionId: "one",
      directories: { "/": { directory: directory("/", ["/a", "/b"]) } },
      expanded: ["/"],
      selected: "/",
      currentPath: "/",
      homePath: "/",
      scrollTop: 0,
    });
    const automatic = model.revealPath("/a");
    await tick();
    model.select("/b", true);
    pending.resolve(batch("/a", ["/a"]));
    await automatic;
    expect(model.getState().selected).toBe("/b");
    expect(model.getState().currentPath).toBe("/b");
    expect(model.getState().expanded).toEqual(["/"]);
    expect(model.getState().revealing).toBe(false);
    expect(model.getState().directories["/a"].directory).toBeDefined();
  });
});
