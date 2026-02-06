//! Feedback + optimization integration.

use horizons_core::evaluation_traits::VerificationCase;
use horizons_core::optimization_traits::{ContinualLearning, Dataset, MiproConfig, Policy};
use ulid::Ulid;

/// Build a verification case from user feedback (reaction).
#[allow(dead_code)]
pub fn build_verification_case(
    user_input: &str,
    assistant_output: &str,
    positive: bool,
) -> VerificationCase {
    VerificationCase {
        id: Ulid::new(),
        input: user_input.to_string(),
        output: assistant_output.to_string(),
        expected: if positive {
            Some("good".to_string())
        } else {
            Some("poor".to_string())
        },
        metadata: serde_json::json!({}),
    }
}

/// Placeholder for MIPRO optimization batch.
#[allow(dead_code)]
pub async fn run_optimization_batch(
    learner: &dyn ContinualLearning,
    current_prompt: &str,
    dataset: Dataset,
) -> Result<String, String> {
    let cfg = MiproConfig::default();
    let policy = Policy {
        template: current_prompt.to_string(),
        metadata: serde_json::json!({}),
    };

    match learner.run_batch(cfg, policy, dataset).await {
        Ok(result) => Ok(result.best_policy.template),
        Err(e) => Err(format!("optimization failed: {e}")),
    }
}
