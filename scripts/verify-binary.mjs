import fs from "node:fs";
export function verifyBinary(file, target) {
  const bytes = fs.readFileSync(file);
  const arm = target.startsWith("aarch64");
  let actual;
  if (target.includes("windows")) {
    const offset = bytes.readUInt32LE(0x3c);
    if (bytes.toString("ascii", offset, offset + 4) !== "PE\0\0")
      throw new Error("Not PE");
    actual = bytes.readUInt16LE(offset + 4);
    if (actual !== (arm ? 0xaa64 : 0x8664))
      throw new Error("Wrong PE architecture");
  } else if (target.includes("linux")) {
    if (bytes.toString("ascii", 1, 4) !== "ELF" || bytes[4] !== 2)
      throw new Error("Not ELF64");
    actual = bytes.readUInt16LE(18);
    if (actual !== (arm ? 183 : 62)) throw new Error("Wrong ELF architecture");
  } else {
    if (
      bytes.readUInt32LE(0) !== 0xfeedfacf ||
      bytes.readUInt32LE(4) !== 0x100000c
    )
      throw new Error("Not native ARM64 Mach-O");
  }
  console.log(`Verified native binary: ${target}`);
}
if (process.argv[1]?.replaceAll("\\", "/").endsWith("/verify-binary.mjs"))
  verifyBinary(process.argv[2], process.argv[3]);
