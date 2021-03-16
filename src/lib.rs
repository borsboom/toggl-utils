pub mod ontrack;

pub fn cargo_crate_name() -> &'static str {
    env!("CARGO_CRATE_NAME")
}
