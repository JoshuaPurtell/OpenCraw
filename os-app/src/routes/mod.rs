pub mod automation;
pub mod channels;
pub mod config;
pub mod health;
pub mod memory;
pub mod messages;
pub mod sessions;
pub mod skills;

use axum::Router;

pub fn router() -> Router {
    Router::new()
        .merge(health::router())
        .merge(config::router())
        .merge(automation::router())
        .merge(channels::router())
        .merge(memory::router())
        .merge(sessions::router())
        .merge(messages::router())
        .merge(skills::router())
}
