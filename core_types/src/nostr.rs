use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct NostrProfile {
    banner: Option<String>,
    website: Option<String>,
    lud06: Option<String>,
    nip05: Option<String>,
    picture: Option<String>,
    display_name: Option<String>,
    about: Option<String>,
    name: Option<String>,
    lud16: Option<String>,
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
}
