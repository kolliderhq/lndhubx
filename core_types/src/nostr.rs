use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct NostrProfile {
    banner: Option<String>,
    webiste: Option<String>,
    lud06: Option<String>,
    nip05: Option<String>,
    picture: Option<String>,
    display_name: Option<String>,
    about: Option<String>,
    name: Option<String>,
}
