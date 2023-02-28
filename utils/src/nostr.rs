use nostr_sdk::nostr::event;

pub const ZAP_REQUEST_MEMO: &str = "Zap Request";

const ZAP_REQUEST_KIND: u64 = 9734;
const ZAP_NOTE_KIND: u64 = 9735;

#[derive(Debug)]
pub enum ZapError {
    InvalidJson,
    InvalidSignature,
    NotZapRequest,
    NoTags,
    NoPubkeyTag,
    InvalidEventTagCount,
    NoRelays,
    AmountMismatch,
    DescriptionHashMismatch,
    CouldNotCreateZapNote,
}

pub fn validate_zap_request(json: &str, invoice_amount: u64) -> Result<nostr_sdk::nostr::Event, ZapError> {
    let nostr_event = nostr_sdk::nostr::Event::from_json(json).map_err(|err| match err {
        event::Error::InvalidSignature | event::Error::Secp256k1(_) => ZapError::InvalidSignature,
        event::Error::Json(_) | event::Error::Hex(_) => ZapError::InvalidJson,
    })?;

    if event::Kind::Custom(ZAP_REQUEST_KIND) != nostr_event.kind {
        return Err(ZapError::NoTags);
    }

    if nostr_event.tags.is_empty() {
        return Err(ZapError::NoTags);
    }

    let mut pubkey_tags_count = 0;
    let mut event_tags_count = 0;
    let mut relays = Vec::new();
    let mut amount = Option::<u64>::default();
    for tag in nostr_event.tags.iter() {
        match tag {
            event::Tag::PubKey(_, _) => {
                pubkey_tags_count += 1;
            }
            event::Tag::Event(_, _, _) => {
                event_tags_count += 1;
            }
            event::Tag::Generic(event::TagKind::Custom(tag), values) => match tag.as_str() {
                "relays" => {
                    relays = values.clone();
                }
                "amount" => {
                    amount = values.first().and_then(|v| v.parse().ok());
                }
                _ => {}
            },
            _ => {}
        }
    }

    if pubkey_tags_count < 1 {
        return Err(ZapError::NoPubkeyTag);
    }

    if event_tags_count > 1 {
        return Err(ZapError::InvalidEventTagCount);
    }

    if relays.is_empty() {
        return Err(ZapError::NoRelays);
    }

    if let Some(value) = amount {
        if value != invoice_amount {
            return Err(ZapError::AmountMismatch);
        }
    }

    Ok(nostr_event)
}

pub fn create_zap_note(
    keys: &nostr_sdk::nostr::Keys,
    amount: u64,
    description: &str,
    description_hash: &str,
    bolt11: &str,
    preimage: &str,
    settled_timestamp: u64,
) -> Result<(nostr_sdk::nostr::Event, Vec<String>), ZapError> {
    let description_sha256 = sha256::digest(description);

    if description_hash != description_sha256 {
        return Err(ZapError::DescriptionHashMismatch);
    }

    let request = validate_zap_request(description, amount)?;

    let relays = request
        .tags
        .iter()
        .filter_map(|tag| {
            if let event::Tag::Generic(event::TagKind::Custom(custom_tag), values) = tag {
                if custom_tag == "relays" {
                    Some(values.clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .next()
        .expect("Relays should exist as request validation passed");

    let bolt11_tag = event::Tag::Generic(
        event::TagKind::Custom(String::from("bolt11")),
        vec![String::from(bolt11)],
    );
    let description_tag = event::Tag::Generic(
        event::TagKind::Custom(String::from("description")),
        vec![String::from(description)],
    );
    let preimage_tag = event::Tag::Generic(
        event::TagKind::Custom(String::from("preimage")),
        vec![String::from(preimage)],
    );
    let tags = request
        .tags
        .into_iter()
        .filter(|tag| matches!(tag, event::Tag::PubKey(_, _) | event::Tag::Event(_, _, _)))
        .chain([bolt11_tag, description_tag, preimage_tag])
        .collect::<Vec<_>>();
    let builder = event::EventBuilder::new(event::Kind::Custom(ZAP_NOTE_KIND), "", tags.as_slice());
    let mut event = builder.to_event(keys).map_err(|_| ZapError::CouldNotCreateZapNote)?;
    event.created_at = nostr_sdk::nostr::Timestamp::from(settled_timestamp);
    Ok((event, relays))
}
