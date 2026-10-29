use super::*;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct SignatureStatus {
    pub status: String,
    #[serde(skip_serializing_if="Option::is_none")]
    pub key_id: Option<String>,
}
pub(super) fn evaluate(bytes: Option<&[u8]>, integrity: &[u8], keys: &TrustedKeys) -> Result<SignatureStatus> {
    let Some(bytes) = bytes else { return Ok(SignatureStatus { status: "unsigned".into(), key_id: None }); };
    let bad = || Error::Fault("PACKAGE_SIGNATURE_INVALID", "签名文件无效".into());
    if bytes.len() > 16*1024 { return Err(bad()); }
    let signature: Signature = serde_json::from_slice(bytes).map_err(|_| bad())?;
    if signature.key_id.is_empty() || signature.key_id.len()>128 || signature.key_id.chars().any(char::is_control) { return Err(bad()); }
    let decoded = base64::engine::general_purpose::STANDARD.decode(&signature.signature).map_err(|_| bad())?;
    if decoded.len()!=64 { return Err(bad()); }
    let status = if let Some(key) = keys.get(&signature.key_id) {
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key.public_key)
            .verify(integrity, &decoded)
            .map_err(|_| Error::Fault("PACKAGE_SIGNATURE_INVALID", "签名校验失败".into()))?;
        if key.development_only { "developmentVerified" } else { "verified" }
    } else { "unverified" };
    Ok(SignatureStatus {status:status.into(), key_id:Some(signature.key_id)})
}
