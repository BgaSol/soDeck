use serde_json::Value;
use socker::socks5::Reply;
use sodeck_plugin_client::{
    protocol::network::{FailureCode as Code, FailureStage as Stage, NetworkFailure},
    RpcError,
};

pub fn protocol(message: impl AsRef<str>) -> NetworkFailure {
    NetworkFailure::new(Code::ProtocolError, Stage::Negotiation, message.as_ref())
}
pub fn rpc(error: RpcError) -> NetworkFailure {
    NetworkFailure::new(
        Code::Unknown,
        Stage::Negotiation,
        "核心网络请求未能确认结果",
    )
    .with_diagnostic(
        serde_json::json!({"code":error.code,"message":error.message,"data":error.data}),
    )
}
pub fn read(value: &Value) -> NetworkFailure {
    serde_json::from_value(value["failure"].clone()).unwrap_or_else(|error| {
        protocol("核心网络故障回执无效，无法确认具体原因").with_diagnostic(error)
    })
}
pub fn socks_reply(failure: &NetworkFailure) -> Reply {
    match failure.code {
        Code::ForwardingDenied => Reply::from(2),
        Code::ConnectionRefused => Reply::CONNECTION_REFUSED,
        Code::Timeout => Reply::TTL_EXPIRED,
        _ => Reply::GENERAL_FAILURE,
    }
}
pub fn socks_failure(rep: u8) -> NetworkFailure {
    let (code, message) = match rep {
        2 => (Code::ForwardingDenied, "SOCKS5 代理拒绝访问此目标"),
        3 => (Code::TargetConnectFailed, "SOCKS5 代理无法到达目标网络"),
        4 => (Code::TargetConnectFailed, "SOCKS5 代理无法到达目标主机"),
        5 => (Code::ConnectionRefused, "目标连接被拒绝"),
        6 => (Code::Timeout, "SOCKS5 目标连接超时"),
        7 => (Code::ForwardingUnsupported, "SOCKS5 代理不支持此命令"),
        8 => (Code::ProtocolError, "SOCKS5 代理不支持此地址类型"),
        _ => (Code::Unknown, "SOCKS5 目标连接失败"),
    };
    NetworkFailure::new(code, Stage::Check, message)
        .with_hint("请检查代理策略和从远端访问目标的连通性；SOCKS5 回复不能确定具体的 SSH 配置。")
        .with_diagnostic(format!("SOCKS5 REP {rep}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socks_codes_do_not_depend_on_error_words() {
        for (code, rep) in [
            (Code::ForwardingDenied, 2),
            (Code::ConnectionRefused, 5),
            (Code::Timeout, 6),
            (Code::TargetConnectFailed, 1),
            (Code::Unknown, 1),
        ] {
            let failure = NetworkFailure::new(code, Stage::TargetConnect, "refused 超时");
            assert_eq!(u8::from(socks_reply(&failure)), rep);
        }
        let failure = socks_failure(2);
        assert_eq!(failure.code, Code::ForwardingDenied);
        assert!(!failure.message.contains("SSH"));
        assert_eq!(
            read(&serde_json::json!({"error":"AdministrativelyProhibited"})).code,
            Code::ProtocolError
        );
    }
}
