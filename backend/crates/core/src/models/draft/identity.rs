use uuid::Uuid;

const SCEPA_ID_NAMESPACE: Uuid = Uuid::from_u128(0x59f13bb5_524f_5e53_8b30_61e9ef3bcce8);

pub(crate) fn stable_id(scope: &str, kind: &str, locator: &str) -> String {
    Uuid::new_v5(
        &SCEPA_ID_NAMESPACE,
        format!("{scope}\0{kind}\0{locator}").as_bytes(),
    )
    .to_string()
}

pub(crate) fn is_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok()
}
