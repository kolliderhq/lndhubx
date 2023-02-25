use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct NostrProfile {
    #[serde(default)]
    banner: Option<String>,
    #[serde(default)]
    website: Option<String>,
    #[serde(default)]
    lud06: Option<String>,
    #[serde(default)]
    nip05: Option<String>,
    #[serde(default)]
    picture: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    about: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    lud16: Option<String>,
    #[serde(default)]
    nip05_verified: Option<bool>,
}

impl NostrProfile {
    pub fn name(&self) -> &Option<String> {
        &self.name
    }

    pub fn display_name(&self) -> &Option<String> {
        &self.display_name
    }

    pub fn nip05(&self) -> &Option<String> {
        &self.nip05
    }

    pub fn lud06(&self) -> &Option<String> {
        &self.lud06
    }

    pub fn lud16(&self) -> &Option<String> {
        &self.lud16
    }

    pub fn nip05_verified(&self) -> Option<bool> {
        self.nip05_verified
    }

    pub fn set_nip05_verified(&mut self, verified: Option<bool>) {
        self.nip05_verified = verified;
    }
}
