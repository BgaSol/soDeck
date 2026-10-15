import type { TransferAuth } from "./transferApi";

export function TransferAuthFields({
  value,
  onChange,
  disabled = false,
  saved = true,
}: {
  value: TransferAuth;
  onChange: (value: TransferAuth) => void;
  disabled?: boolean;
  saved?: boolean;
}) {
  return (
    <fieldset disabled={disabled} className="transfer-auth-fields">
      <legend>源服务器如何登录目标服务器</legend>
      <label>
        直传认证方式
        <select
          aria-label="直传认证方式"
          value={value.mode}
          onChange={(e) =>
            onChange({ mode: e.target.value as TransferAuth["mode"] })
          }
        >
          {saved && <option value="savedPassword">使用目标已保存的密码</option>}
          <option value="password">输入本次密码</option>
          <option value="key">使用源服务器上的私钥</option>
        </select>
      </label>
      {value.mode === "password" && (
        <label>
          本次密码
          <input
            type="password"
            aria-label="直传本次密码"
            autoComplete="new-password"
            value={value.password ?? ""}
            onChange={(e) => onChange({ ...value, password: e.target.value })}
          />
        </label>
      )}
      {value.mode === "key" ? (
        <>
          <label>
            源服务器上的私钥路径
            <input
              aria-label="源服务器私钥路径"
              placeholder="/home/user/.ssh/id_ed25519"
              value={value.keyPath ?? ""}
              onChange={(e) => onChange({ ...value, keyPath: e.target.value })}
            />
          </label>
          <p className="muted">
            使用 A 上可非交互读取的私钥。本机私钥和 Agent 不会自动转发。
          </p>
        </>
      ) : (
        <p className="muted">
          A 将在本次传输中使用 B 的密码；A 需要安装
          sshpass。密码不写入命令参数或任务记录。
        </p>
      )}
      <p className="muted">直传要求 A、B 均有 rsync，B 支持 SSH 与 SFTP。</p>
    </fieldset>
  );
}
