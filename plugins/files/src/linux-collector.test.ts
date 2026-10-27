import { describe, expect, it, vi } from "vitest";
import { Remote } from "./remote";
import { mounts, mountCapacity, type MountSnapshot } from "./linux-collector";
import { parseMounts } from "./mounts";
import { path, pathText, root, utf8 } from "./remote-path";

const table = "1 0 8:1 / / rw - ext4 /dev/root rw\n";
const stat = (start = "1234") => `42 (sftp worker) S ${Array(18).fill("0").join(" ")} ${start} 0\n`;
const fd = (inode = "2", mount = "1") => `pos:\t0\nflags:\t0100000\nmnt_id:\t${mount}\nino:\t${inode}\n`;
const ns = "mnt:[4026531841]";
function remote() {
  return new Remote({ async call() { throw Error("Unexpected bridge call"); } }, {
    workspaceId: "w", workspaceGeneration: "g",
  });
}
function mountFixture() {
  const r = remote();
  const parts = [stat(), ns + "\n", "/\n", fd(), table, table, fd(), ns + "\n", stat()];
  const changes = { afterStat: stat(), afterTable: table, afterFd: fd(), afterNs: ns, afterRoot: "/" };
  const reads = new Map<string, number>();
  vi.spyOn(r, "openDirectory").mockImplementation(async p => ({
    directoryId: pathText(p) === "/" ? "root-handle" : "fd-list", cursor: 0,
  }));
  vi.spyOn(r, "page").mockResolvedValue({entries:[{
    name:{bytesBase64:btoa("9")},
    attributes:{type:"symlink",size:null,mtime:null,atime:null,permissions:null,uid:null,gid:null},
  }],nextCursor:1,eof:true});
  vi.spyOn(r, "call").mockImplementation(async (_m, p) => {
    const name=pathText(p!.path as ReturnType<typeof path>);
    const count=(reads.get(name)??0)+1;reads.set(name,count);
    return {path:{bytesBase64:btoa(name.endsWith("ns/mnt")
      ? (count===1?ns:changes.afterNs) : (count===1?"/":changes.afterRoot))}};
  });
  vi.spyOn(r, "readFile").mockImplementation(async p => {
    const name=pathText(p), count=(reads.get(name)??0)+1;reads.set(name,count);
    if(name.endsWith("/stat"))return utf8(count===1?stat():changes.afterStat);
    if(name.endsWith("/mountinfo"))return utf8(count===1?table:changes.afterTable);
    if(name.endsWith("/fdinfo/9"))return utf8(count===1?fd():changes.afterFd);
    throw Error("Unexpected file: "+name);
  });
  vi.spyOn(r, "exec").mockImplementation(async()=>utf8(parts.join("\nSODECK_FILES_SECTION\n")));
  vi.spyOn(r, "close").mockResolvedValue();
  return {r,parts,changes};
}

describe("procfs root correspondence", () => {
  it("verifies both pinned roots without GNU stat and releases handles", async () => {
    const {r}=mountFixture();
    expect((await mounts(r)).mounts[0].source).toBe("/dev/root");
    const command=vi.mocked(r.exec).mock.calls[0][0];
    expect(command).toContain("exec 9< /");
    expect(command).toContain("cat /proc/$$/fdinfo/9");
    expect(command).not.toContain("stat -");
    expect(r.close).toHaveBeenCalledWith("fd-list");
    expect(r.close).toHaveBeenCalledWith("root-handle");
  });
  it.each([
    [1,"mnt:[5]\n"], [2,"/different\n"], [3,fd("3")], [3,fd("2","7")],
    [3,"mnt_id: 1\n"], [4,table+"\n"], [5,table+"\n"], [6,fd("3")],
    [7,"mnt:[6]\n"], [8,stat("5678")],
  ] as const)("rejects mismatched or missing exec proof %s",async(index,value)=>{
    const {r,parts}=mountFixture();parts[index]=value;
    await expect(mounts(r)).rejects.toMatchObject({code:"MOUNT_UNAVAILABLE"});
    expect(r.close).toHaveBeenCalledWith("root-handle");
  });
  it.each([
    ["afterStat",stat("5678")], ["afterTable",table+"\n"], ["afterFd",fd("3")],
    ["afterNs","mnt:[7]"], ["afterRoot","/changed"],
  ] as const)("rejects changed SFTP proof %s",async(key,value)=>{
    const {r,changes}=mountFixture();changes[key]=value;
    await expect(mounts(r)).rejects.toMatchObject({code:"MOUNT_UNAVAILABLE"});
    expect(r.close).toHaveBeenCalledWith("root-handle");
  });
  it.each(["EXEC_FAILED","CANCELED"])("releases the root when collection fails: %s",async code=>{
    const {r}=mountFixture();vi.mocked(r.exec).mockRejectedValue({code});
    await expect(mounts(r)).rejects.toMatchObject({code});
    expect(r.close).toHaveBeenCalledWith("root-handle");
  });
  it("releases handles if procfs enumeration fails",async()=>{
    const {r}=mountFixture();vi.mocked(r.page).mockRejectedValue({code:"PERMISSION_DENIED"});
    await expect(mounts(r)).rejects.toMatchObject({code:"PERMISSION_DENIED"});
    expect(r.close).toHaveBeenCalledWith("fd-list");
    expect(r.close).toHaveBeenCalledWith("root-handle");
  });
});

const snapshot: MountSnapshot={mounts:parseMounts(utf8(table)),raw:utf8(table),observedAt:0,pid:"42",start:"1234"};
function capacityFixture(output:string) {
  const r=remote();vi.spyOn(r,"readFile").mockResolvedValue(utf8(table));
  vi.spyOn(r,"exec").mockResolvedValue(utf8(output));return r;
}
describe("capacity collection",()=>{
  it("accepts stat and uses an inert path with one bounded exec",async()=>{
    const r=capacityFixture("SODECK_STAT\n4096 100 40 30\n");
    const cap=await mountCapacity(r,path("/x'$(touch nope)\n"),snapshot);
    expect(cap).toMatchObject({source:"stat",total:"409600",free:"163840"});
    const [command,timeout]=vi.mocked(r.exec).mock.calls[0];
    expect(command).not.toContain("touch nope");expect(command).toContain("df -Pk -- \"$p\"");
    expect(timeout).toBe(10000);expect(r.exec).toHaveBeenCalledTimes(1);
  });
  it("accepts df mode while leaving unsupported free space unavailable",async()=>{
    const r=capacityFixture("SODECK_DF\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/root 1000 250 700 27% /\n");
    expect(await mountCapacity(r,root,snapshot)).toMatchObject({source:"df",free:null,total:"1024000",percent:"25.0%"});
  });
  it.each(["CANCELED","TIMEOUT","EXEC_FAILED"])("does not retry after %s",async code=>{
    const r=capacityFixture("");vi.mocked(r.exec).mockRejectedValue({code});
    await expect(mountCapacity(r,root,snapshot)).rejects.toMatchObject({code});
    expect(r.exec).toHaveBeenCalledTimes(1);
  });
  it.each([0,1])("rejects changed mounts before/after capacity: %s",async when=>{
    const r=capacityFixture("SODECK_STAT\n4096 100 40 30\n");
    vi.mocked(r.readFile).mockReset();
    if(when)vi.mocked(r.readFile).mockResolvedValueOnce(utf8(table));
    vi.mocked(r.readFile).mockResolvedValue(utf8(table+"\n"));
    await expect(mountCapacity(r,root,snapshot)).rejects.toMatchObject({code:"MOUNT_CHANGED"});
    expect(r.exec).toHaveBeenCalledTimes(when);
  });
});
