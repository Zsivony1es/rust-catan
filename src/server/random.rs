//! Seedable RNG helpers so dice and shuffles are reproducible in tests.
//!
//! `SERVER_SEED` (a `u64`) fixes the seed for every game; otherwise each
//! game gets fresh entropy. `POST /games` also accepts an explicit seed.

use rand::{RngExt as _, SeedableRng as _};
use rand_chacha::ChaCha8Rng;

/// Build a game RNG: explicit seed > `SERVER_SEED` env > fresh entropy.
pub fn game_rng(seed: Option<u64>) -> ChaCha8Rng {
    if let Some(seed) = seed {
        return ChaCha8Rng::seed_from_u64(seed);
    }
    if let Ok(raw) = std::env::var("SERVER_SEED") {
        if let Ok(seed) = raw.parse::<u64>() {
            return ChaCha8Rng::seed_from_u64(seed);
        }
    }
    ChaCha8Rng::seed_from_u64(rand::rng().random())
}
