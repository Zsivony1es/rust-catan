//! Typed HTTP wrapper over the v1 API: token store + polling helpers.

use reqwest::header;

use crate::model::{EdgeId, IntersectionId};
use crate::server::dto::{
    BuildRequest, DevPlayRequest, DiscardRequest, ErrorBody, HealthResponse, JoinRequest,
    JoinResponse, LogResponse, RobberRequest, RollResponse, SetupRequest, TradeRequest,
    TradeResponse,
};
use crate::{BuildKind, DevCardKind, GameEvent, GameSession, PlayerColor, ResourceBag, TradeKind};

/// API errors surfaced to the strategy / main loop.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("server {status}: {message}")]
    Server { status: u16, message: String },
}

/// Thin client for one game + one color.
#[derive(Debug, Clone)]
pub struct Api {
    http: reqwest::Client,
    base_url: String,
    pub game_id: String,
    pub color: PlayerColor,
    token: Option<String>,
}

impl Api {
    pub fn new(
        base_url: impl Into<String>,
        game_id: impl Into<String>,
        color: PlayerColor,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
            game_id: game_id.into(),
            color,
            token: None,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn game_path(&self, suffix: &str) -> String {
        format!("/games/{}{suffix}", self.game_id)
    }

    fn authed(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            request.header(header::AUTHORIZATION, format!("Bearer {token}"))
        } else {
            request
        }
    }

    async fn check(response: reqwest::Response) -> Result<reqwest::Response, ApiError> {
        let status = response.status().as_u16();
        if response.status().is_success() {
            return Ok(response);
        }
        let message = response
            .json::<ErrorBody>()
            .await
            .map(|b| b.error.message)
            .unwrap_or_else(|_| "unknown error".into());
        Err(ApiError::Server { status, message })
    }

    // -- lobby -----------------------------------------------------------

    pub async fn health(&self) -> Result<HealthResponse, ApiError> {
        let response = Self::check(self.http.get(self.url("/health")).send().await?).await?;
        Ok(response.json().await?)
    }

    pub async fn create_game(base_url: &str, seed: Option<u64>) -> Result<GameSession, ApiError> {
        let http = reqwest::Client::new();
        let response = Self::check(
            http.post(format!("{base_url}/games"))
                .json(&serde_json::json!({ "seed": seed }))
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    pub async fn join(&mut self) -> Result<GameSession, ApiError> {
        let response = Self::check(
            self.http
                .post(self.url(&self.game_path("/join")))
                .json(&JoinRequest { color: self.color })
                .send()
                .await?,
        )
        .await?;
        let joined: JoinResponse = response.json().await?;
        self.token = Some(joined.token);
        Ok(joined.session)
    }

    pub async fn start(&self) -> Result<GameSession, ApiError> {
        let response = Self::check(
            self.http
                .post(self.url(&self.game_path("/start")))
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    // -- reads ------------------------------------------------------------

    pub async fn state(&self) -> Result<GameSession, ApiError> {
        let response = Self::check(
            self.http
                .get(self.url(&self.game_path("/state")))
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    pub async fn log(&self) -> Result<Vec<GameEvent>, ApiError> {
        let response = Self::check(
            self.http
                .get(self.url(&self.game_path("/log")))
                .send()
                .await?,
        )
        .await?;
        Ok(response.json::<LogResponse>().await?.events)
    }

    // -- setup / turn engine ----------------------------------------------

    pub async fn setup(
        &self,
        intersection_id: IntersectionId,
        edge_id: EdgeId,
    ) -> Result<GameSession, ApiError> {
        let color = self.color;
        let response = Self::check(
            self.authed(self.http.post(self.url(&self.game_path("/setup"))))
                .json(&SetupRequest {
                    color,
                    intersection_id,
                    edge_id,
                })
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    pub async fn roll(&self) -> Result<RollResponse, ApiError> {
        let response = Self::check(
            self.authed(self.http.post(self.url(&self.game_path("/roll"))))
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    pub async fn discard(&self, cards: &ResourceBag) -> Result<GameSession, ApiError> {
        let color = self.color;
        let response = Self::check(
            self.authed(self.http.post(self.url(&self.game_path("/discard"))))
                .json(&DiscardRequest {
                    color,
                    cards: cards.clone(),
                })
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    pub async fn robber(
        &self,
        hex_id: u32,
        victim: Option<PlayerColor>,
    ) -> Result<GameSession, ApiError> {
        let response = Self::check(
            self.authed(self.http.post(self.url(&self.game_path("/robber"))))
                .json(&RobberRequest { hex_id, victim })
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    // -- actions -----------------------------------------------------------

    pub async fn build_road(&self, edge_id: EdgeId) -> Result<GameSession, ApiError> {
        self.build(BuildKind::Road, Some(edge_id), None).await
    }

    pub async fn build_settlement(
        &self,
        intersection_id: IntersectionId,
    ) -> Result<GameSession, ApiError> {
        self.build(BuildKind::Settlement, None, Some(intersection_id))
            .await
    }

    pub async fn build_city(
        &self,
        intersection_id: IntersectionId,
    ) -> Result<GameSession, ApiError> {
        self.build(BuildKind::City, None, Some(intersection_id))
            .await
    }

    pub async fn buy_dev(&self) -> Result<GameSession, ApiError> {
        self.build(BuildKind::DevCard, None, None).await
    }

    async fn build(
        &self,
        kind: BuildKind,
        edge_id: Option<EdgeId>,
        intersection_id: Option<IntersectionId>,
    ) -> Result<GameSession, ApiError> {
        let response = Self::check(
            self.authed(self.http.post(self.url(&self.game_path("/build"))))
                .json(&BuildRequest {
                    kind,
                    edge_id,
                    intersection_id,
                })
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    pub async fn trade_bank(
        &self,
        give: &ResourceBag,
        want: &ResourceBag,
    ) -> Result<GameSession, ApiError> {
        Ok(self.trade(TradeKind::Bank, give, want, None).await?.session)
    }

    pub async fn trade_port(
        &self,
        give: &ResourceBag,
        want: &ResourceBag,
    ) -> Result<GameSession, ApiError> {
        Ok(self.trade(TradeKind::Port, give, want, None).await?.session)
    }

    pub async fn propose_trade(
        &self,
        to: PlayerColor,
        give: &ResourceBag,
        want: &ResourceBag,
    ) -> Result<TradeResponse, ApiError> {
        self.trade(TradeKind::Player, give, want, Some(to)).await
    }

    async fn trade(
        &self,
        kind: TradeKind,
        give: &ResourceBag,
        want: &ResourceBag,
        to: Option<PlayerColor>,
    ) -> Result<TradeResponse, ApiError> {
        let response = Self::check(
            self.authed(self.http.post(self.url(&self.game_path("/trade"))))
                .json(&TradeRequest {
                    kind,
                    give: give.clone(),
                    want: want.clone(),
                    to,
                })
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    pub async fn accept_trade(&self, trade_id: &str) -> Result<GameSession, ApiError> {
        let response = Self::check(
            self.authed(
                self.http
                    .post(self.url(&self.game_path(&format!("/trade/{trade_id}/accept")))),
            )
            .send()
            .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    pub async fn decline_trade(&self, trade_id: &str) -> Result<GameSession, ApiError> {
        let response = Self::check(
            self.authed(
                self.http
                    .post(self.url(&self.game_path(&format!("/trade/{trade_id}/decline")))),
            )
            .send()
            .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    pub async fn play_dev(&self, request: DevPlayRequest) -> Result<GameSession, ApiError> {
        let response = Self::check(
            self.authed(self.http.post(self.url(&self.game_path("/dev/play"))))
                .json(&request)
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }

    pub async fn play_knight(
        &self,
        hex_id: u32,
        victim: Option<PlayerColor>,
    ) -> Result<GameSession, ApiError> {
        self.play_dev(DevPlayRequest {
            kind: DevCardKind::Knight,
            hex_id: Some(hex_id),
            victim,
            ..Default::default()
        })
        .await
    }

    pub async fn end_turn(&self) -> Result<GameSession, ApiError> {
        let response = Self::check(
            self.authed(self.http.post(self.url(&self.game_path("/end-turn"))))
                .send()
                .await?,
        )
        .await?;
        Ok(response.json().await?)
    }
}
