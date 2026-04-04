mod normalize;
mod schema;

pub use normalize::{NormalizedConfig, NormalizedHook, PlatformRule, parse_and_normalize};

#[cfg(test)]
mod tests;
