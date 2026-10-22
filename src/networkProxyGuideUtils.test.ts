import { describe, expect, it } from "vitest";
import { curlExamples, proxyAddresses } from "./networkProxyGuideUtils";

describe("network proxy guide", () => {
  it("formats IPv4 and IPv6 proxy addresses", () => {
    expect(proxyAddresses("127.0.0.1:50185")).toEqual({
      http: "http://127.0.0.1:50185",
      socks5: "socks5://127.0.0.1:50185",
      socks5h: "socks5h://127.0.0.1:50185",
    });
    expect(proxyAddresses("[::1]:50185").socks5h).toBe("socks5h://[::1]:50185");
  });

  it("quotes targets for both command shells", () => {
    const result = curlExamples("socks5h://127.0.0.1:50185", "http://service/it's-ok");
    expect(result.windows).toContain("curl.exe");
    expect(result.windows).toContain("'http://service/it''s-ok'");
    expect(result.posix).toContain("curl --noproxy \"\"");
    expect(result.posix).toContain("'http://service/it'\\''s-ok'");
  });
});
