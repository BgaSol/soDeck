import { describe, expect, it } from "vitest";
import { addTag, emptyServer, filterServers } from "./types";
describe("服务器筛选与标签规则", () => {
  it("组合名称搜索和标签，保持相同地址的不同用户配置", () => {
    const values = [
      {
        ...emptyServer(),
        id: "a",
        name: "Web-02",
        host: "10.0.0.1",
        username: "ops",
        tags: ["生产"],
      },
      {
        ...emptyServer(),
        id: "b",
        name: "Web-01",
        host: "10.0.0.1",
        username: "root",
        tags: ["测试"],
      },
    ];
    expect(filterServers(values, " web ", "生产").map((s) => s.id)).toEqual([
      "a",
    ]);
    expect(filterServers(values, "10.0.0.1", "")).toHaveLength(2);
  });
  it("去空白和重复，复用已有标签大小写", () => {
    expect(addTag(["Web"], " web ", [])).toEqual(["Web"]);
    expect(addTag([], " prod ", ["Prod"])).toEqual(["Prod"]);
    expect(addTag([], " ", [])).toEqual([]);
  });
});
