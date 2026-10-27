import { describe, it, expect } from "vitest";
import {
  encode,
  decode,
  utf8,
  path,
  join,
  pathText,
  contains,
  posixPath,
  ancestors,
} from "./remote-path";
import { size, capacity, dfCapacity } from "./format";
import { parseMounts, matching, safeOptions, safeSource } from "./mounts";
describe("remote byte identity", () => {
  it("round-trips backslashes, newlines and invalid UTF-8 without creating separators", () => {
    const bytes = Uint8Array.of(0xff, 92, 10, 39, 32, 0xc3, 0xa9);
    expect(decode(encode(bytes))).toEqual(bytes);
    const p = join(path("/data"), { bytesBase64: encode(bytes) });
    expect([...decode(p.bytesBase64)]).toEqual([...utf8("/data/"), ...bytes]);
    expect(pathText(p)).toBe("/data/\\xff\\\\x0a' é");
    expect(contains(path("/data"), path("/database"))).toBe(false);
  });
  it("keeps command arguments inert and preserves trailing newline with a sentinel", () => {
    const command = posixPath(path("/x'$(touch /tmp/unsafe)\n"));
    expect(command).not.toContain("touch");
    expect(command).toContain("\\012x");
    expect(command).toContain("p=" + "$" + "{p%x}");
    expect(ancestors(path("/one/two")).map(pathText)).toEqual([
      "/",
      "/one",
      "/one/two",
    ]);
  });
});
describe("size and filesystem capacity", () => {
  it("preserves values above JS safe integer and distinguishes zero from unknown", () => {
    expect(size("0")).toBe("0 B");
    expect(size("1023")).toBe("1023 B");
    expect(size("1024")).toBe("1 KiB");
    expect(size(null)).toBe("未知");
    expect(capacity("4096 9007199254740993 20 10").total).toBe(
      "36893488147419107328",
    );
    expect(capacity("4096 0 0 0").percent).toBe(null);
    expect(() => capacity("1 10 20 10")).toThrow();
  });
});
describe("POSIX df capacity", () => {
  const header="Filesystem 1024-blocks Used Available Capacity Mounted on\n";
  it("uses reported KiB values and its own ratio, not df's reserved-space percentage",()=>{
    expect(dfCapacity(header+"/dev/root 1000 250 700 27% /\n","/")).toEqual({
      total:"1024000",used:"256000",free:null,available:"716800",percent:"25.0%",source:"df",
    });
  });
  it("parses numeric columns independently of spaces in the source and target",()=>{
    expect(dfCapacity(header+"server:/export 123 456 50% share 1000 250 700 27% /space 123 456 50% end \n","/space 123 456 50% end ").total).toBe("1024000");
    const raw="/invalid-"+String.fromCharCode(255);
    expect(dfCapacity(header+"/dev/root 1000 250 700 27% "+raw+"\n",raw).used).toBe("256000");
  });
  it("keeps large integers exact and zero totals without a ratio",()=>{
    expect(dfCapacity(header+"/dev/root 9007199254740993 1 2 1% /\n","/").total).toBe("9223372036854776832");
    expect(dfCapacity(header+"tmpfs 0 0 0 0% /\n","/").percent).toBeNull();
  });
  it.each(["-100","-"])("keeps unavailable or negative available blocks unknown: %s",available=>{
    expect(dfCapacity(header+`/dev/root 1000 250 ${available} 100% /\n`,"/").available).toBeNull();
  });
  it.each([
    [header.replace("1024-blocks","512-blocks")+"/dev/root 1000 250 700 27% /\n","/"],
    [header+"/dev/root 1000 250 700 27% /wrong\n","/"],
    [header+"/dev/root 1000 250 700 27% /\nextra\n","/"],
    [header+"/dev/root 1000 250 700 27% /new\nline\n","/new\nline"],
    [header+"/dev/root 10 20 0 100% /\n","/"],
    [header+"/dev/root 10 2 20 10% /\n","/"],
    [header+"/dev/root NaN 2 3 10% /\n","/"],
  ])("rejects ambiguous or invalid output",(output,target)=>{
    expect(()=>dfCapacity(output,target)).toThrow();
  });
});
describe("mount semantics and disclosure", () => {
  const lines =
    [
      "1 0 8:1 / / rw,relatime - ext4 /dev/vda rw",
      "2 1 8:1 /source /data rw - ext4 /dev/vda rw",
      "3 1 8:2 /subvol /data/nested ro - btrfs /dev/vdb ro,subvolid=256,subvol=/private",
      "4 1 0:9 / /space\\040and\\134slash rw - cifs //user:password@nas/share rw,password=hunter2,username=me,vers=3.0",
    ].join("\n") + "\n";
  it("matches segments and longest mount, keeping subvolumes and ambiguous layers honest", () => {
    const mounts = parseMounts(utf8(lines));
    expect(matching(mounts, path("/database")).candidates[0].id).toBe("1");
    expect(matching(mounts, path("/data/nested/file"))).toMatchObject({
      exact: false,
      candidates: [{ id: "3", kind: "Btrfs 文件系统 / 子卷" }],
    });
    expect(matching(mounts, path("/data")).exact).toBe(true);
    const stacked = parseMounts(
      utf8(lines + "5 2 0:10 / /data rw - tmpfs tmpfs rw\n"),
    );
    expect(matching(stacked, path("/data")).candidates).toHaveLength(2);
  });
  it("decodes only kernel field escapes and hides non-whitelisted values before UI", () => {
    const mounts = parseMounts(utf8(lines));
    expect(pathText(mounts[3].target)).toBe("/space and\\slash");
    expect(JSON.stringify(mounts)).not.toMatch(
      /hunter2|password@|username=me|\/private/,
    );
    expect(safeSource("//user:secret@host/share")).toBe(
      "//〈已隐藏〉@host/share",
    );
    expect(safeOptions("rw,password=x,unknown=secret,uid=1000")).toEqual([
      "rw",
      "password=〈已隐藏〉",
      "unknown=〈已隐藏〉",
      "uid=1000",
    ]);
    expect(() => parseMounts(utf8("invalid\n"))).toThrow();
  });
  it("keeps namespace roots as metadata without losing ordinary mounts", () => {
    const mounts = parseMounts(utf8(lines + [
      "5 1 0:4 net:[4026531992] /run/netns/default rw - nsfs nsfs rw",
      "6 1 0:4 mnt:[4026531841] /run/mount-ns rw - nsfs nsfs rw",
      "7 1 0:5 special:[123] /special rw - otherfs otherfs rw",
    ].join("\n") + "\n"));
    expect(mounts).toHaveLength(7);
    expect(matching(mounts, path("/"))).toMatchObject({
      exact: true, candidates: [{ id: "1", fsType: "ext4" }],
    });
    expect(matching(mounts, path("/data/file")).candidates[0].id).toBe("2");
    expect(matching(mounts, path("/run/netns/default"))).toMatchObject({
      exact: true, candidates: [{ id: "5", kind: "虚拟文件系统" }],
    });
    expect(mounts.slice(4).map(m => pathText(m.root))).toEqual([
      "net:[4026531992]", "mnt:[4026531841]", "special:[123]",
    ]);
    expect(mounts[5].kind).toBe("虚拟文件系统");
    expect(mounts[6].kind).not.toMatch(/子目录|绑定|本地/);
    expect(matching(mounts, path("/net:[4026531992]")).candidates[0].id).toBe("1");
  });
  it("preserves escaped root and target bytes independently", () => {
    const escaped = "/a\\040b\\011c\\012d\\134e";
    const [mount] = parseMounts(utf8(`1 0 8:1 ${escaped} ${escaped} rw - ext4 /dev/vda rw\n`));
    expect(decode(mount.root.bytesBase64)).toEqual(utf8("/a b\tc\nd\\e"));
    expect(decode(mount.target.bytesBase64)).toEqual(utf8("/a b\tc\nd\\e"));
  });
  it.each([
    "5 1 0:4 net:[4026531992] relative rw - nsfs nsfs rw\n",
    "5 1 0:4  /run/netns/default rw - nsfs nsfs rw\n",
    "5 1 0:4 net:\\999 /run/netns/default rw - nsfs nsfs rw\n",
    "5 1 0:4 net:[4026531992] /run/\\999 rw - nsfs nsfs rw\n",
    "1 0 0:4 net:[4026531992] /run/netns/default rw - nsfs nsfs rw\n",
  ])("rejects malformed records even when namespace roots are supported: %s", line => {
    expect(() => parseMounts(utf8(lines + line))).toThrow();
  });
});
