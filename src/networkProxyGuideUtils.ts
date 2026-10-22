function hostPort(address: string): string {
  const value = address.trim();
  if (!value) return "127.0.0.1:1080";
  return value;
}

function shellQuote(value: string, shell: "powershell" | "posix") {
  const escaped =
    shell === "powershell"
      ? value.replace(/'/g, "''")
      : value.replace(/'/g, "'\\''");
  return `'${escaped}'`;
}

export function proxyAddresses(address: string) {
  const host = hostPort(address);
  return {
    http: `http://${host}`,
    socks5: `socks5://${host}`,
    socks5h: `socks5h://${host}`,
  };
}

export function curlExamples(proxy: string, target: string) {
  const url = target.trim() || "http://service.internal:8080/";
  return {
    windows: `curl.exe --noproxy "" --proxy ${shellQuote(proxy, "powershell")} ${shellQuote(url, "powershell")}`,
    posix: `curl --noproxy "" --proxy ${shellQuote(proxy, "posix")} ${shellQuote(url, "posix")}`,
  };
}
