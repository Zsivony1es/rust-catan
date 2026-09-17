//! Multi-game server state: one mutex per game behind a read-heavy lock.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rand_chacha::ChaCha8Rng;

use crate::{GameId, GameSession, PlayerColor};

/// Legacy single-game alias target.
pub const DEFAULT_GAME_ID: &str = "default";

/// Mutable per-game data held under a short-lived mutex.
pub struct Game {
    pub session: GameSession,
    /// Join tokens by color (`Authorization: Bearer <token>`).
    pub tokens: HashMap<PlayerColor, String>,
    pub rng: ChaCha8Rng,
}

impl Game {
    /// Look up which color owns `token`.
    #[must_use]
    pub fn color_for_token(&self, token: &str) -> Option<PlayerColor> {
        self.tokens
            .iter()
            .find(|(_, t)| t.as_str() == token)
            .map(|(c, _)| *c)
    }
}

#[derive(Clone, Default)]
pub struct AppState {
    games: Arc<tokio::sync::RwLock<HashMap<GameId, Arc<Mutex<Game>>>>>,
}

impl AppState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            games: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Create a game (fixed board, full supply, shuffled deck).
    pub async fn create_game(&self, seed: Option<u64>) -> Arc<Mutex<Game>> {
        let mut rng = crate::server::random::game_rng(seed);
        let id = uuid::Uuid::new_v4().to_string();
        let session = GameSession::new(id.clone(), &mut rng);
        let game = Arc::new(Mutex::new(Game {
            session,
            tokens: HashMap::new(),
            rng,
        }));
        self.games.write().await.insert(id, game.clone());
        game
    }

    pub async fn get(&self, id: &str) -> Option<Arc<Mutex<Game>>> {
        self.games.read().await.get(id).cloned()
    }

    /// Legacy compat: lazily create the `default` game.
    pub async fn get_or_create_default(&self) -> Arc<Mutex<Game>> {
        if let Some(game) = self.get(DEFAULT_GAME_ID).await {
            return game;
        }
        let mut rng = crate::server::random::game_rng(None);
        let session = GameSession::new(DEFAULT_GAME_ID.into(), &mut rng);
        let game = Arc::new(Mutex::new(Game {
            session,
            tokens: HashMap::new(),
            rng,
        }));
        self.games
            .write()
            .await
            .insert(DEFAULT_GAME_ID.into(), game.clone());
        game
    }
}
