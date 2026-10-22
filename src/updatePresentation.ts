import type { UpdateSnapshot } from "./updateApi";
export const phaseLabel: Record<UpdateSnapshot["phase"], string> = {
  idle: "尚未检查",
  checking: "正在检查更新",
  upToDate: "已是最新版本",
  available: "发现新版本",
  downloading: "正在下载更新",
  verifying: "正在验证更新",
  ready: "更新已准备好",
  preparing: "正在复核安装条件",
  installing: "正在关闭连接并安装",
  failed: "更新未完成",
};
export const impactLabel: Record<string, string> = {
  editors: "请保存并关闭远程文件编辑器",
  fileTasks: "请在文件任务面板处理未结束或待核对的任务",
  transfersFinishing: "文件操作正在收尾，请等待完成",
  dockerOperations: "请在 Docker 面板处理操作",
  netbirdOperations: "请在 NetBird 面板核对操作",
  terminals: "断开终端会话",
  logs: "停止日志接收",
  monitors: "停止服务器监控",
  portForwarding: "停止端口转发与内网代理",
};
export function updateActions(s: UpdateSnapshot | null, main: boolean) {
  const active =
    !!s?.operation ||
    (!!s &&
      [
        "checking",
        "downloading",
        "verifying",
        "preparing",
        "installing",
      ].includes(s.phase));
  const recovery = s?.error?.code === "restartRequired";
  return {
    check: !active && s?.phase !== "ready" && !recovery,
    download:
      !!s?.release &&
      s.install.mode === "inApp" &&
      main &&
      !active &&
      s.phase !== "ready" &&
      !recovery,
    cancel:
      !!s?.operation && main && ["downloading", "verifying"].includes(s.phase),
    preferences:
      !recovery && s?.phase !== "preparing" && s?.phase !== "installing",
    recovery,
  };
}
