use super::*;
pub(super) mod registry;
mod service;
mod appearance;
pub(super) use registry::Registry;
pub(super) fn check(kind:&str,method:&str,value:&Value)->Result<()> {
    if serde_json::to_vec(value).unwrap().len()>64*1024{return Err(err("CAPACITY","设置请求或结果超出容量"));}
    static SCHEMA:std::sync::OnceLock<Value>=std::sync::OnceLock::new();
    let schema=SCHEMA.get_or_init(||serde_json::from_str(include_str!("../../../../../contracts/settings.generated.json")).unwrap());
    let name=schema[kind][method].as_str().ok_or_else(||err("UNSUPPORTED_API","未知设置方法"))?;
    sodeck_core::contract::check(schema,&schema["$defs"][name],value,method).map_err(|e|err(if kind=="x-methods"{"INVALID_PARAMS"}else{"PROTOCOL_ERROR"},e))
}
