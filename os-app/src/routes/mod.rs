pub mod channels;
pub mod health;
pub mod messages;
pub mod sessions;
pub mod skills;

use axum::Router;

pub fn router() -> Router {
    Router::new()
        .merge(health::router())
        .merge(channels::router())
        .merge(sessions::router())
        .merge(messages::router())
        .merge(skills::router())
}
