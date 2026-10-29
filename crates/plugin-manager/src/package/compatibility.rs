use super::{Error, Manifest, Result};

/// An explicit environment to inspect against. It never grants permission to run.
#[derive(Clone, Debug)]
pub struct CompatibilityContext {
    pub core_version: String,
    pub api_version: String,
    pub capabilities: Vec<String>,
    pub platform: String,
}

impl CompatibilityContext {
    pub fn current() -> Self {
        Self {
            core_version: crate::CORE_VERSION.into(),
            api_version: crate::API_VERSION.into(),
            capabilities: crate::CAPABILITIES.iter().map(|v| (*v).into()).collect(),
            platform: std::env::consts::OS.into(),
        }
    }

    pub fn validate(&self, manifest: &Manifest) -> Result<()> {
        let fault = || Error::Fault("PACKAGE_INCOMPATIBLE", "Plugin requires an incompatible runtime".into());
        for (requirement, actual) in [
            (&manifest.requires.core, &self.core_version),
            (&manifest.requires.plugin_api, &self.api_version),
        ] {
            let r = semver::VersionReq::parse(requirement).map_err(|_| fault())?;
            let v = semver::Version::parse(actual).map_err(|_| fault())?;
            if !r.matches(&v) { return Err(fault()); }
        }
        if manifest.requires.capabilities.iter().any(|c| !self.capabilities.contains(c))
            || !manifest.requires.platforms.is_empty()
                && !manifest.requires.platforms.contains(&self.platform) {
            return Err(fault());
        }
        Ok(())
    }
}
