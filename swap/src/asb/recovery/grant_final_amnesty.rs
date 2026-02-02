use crate::protocol::alice::AliceState;
use crate::protocol::Database;
use anyhow::{bail, Result};
use std::convert::TryInto;
use std::sync::Arc;
use uuid::Uuid;

pub async fn grant_final_amnesty(
    swap_id: Uuid,
    db: Arc<dyn Database + Send + Sync>,
) -> Result<AliceState> {
    let state = db.get_state(swap_id).await?.try_into()?;

    match state {
        AliceState::BtcWithholdConfirmed { state3 } => {
            let new_state = AliceState::BtcMercyGranted { state3 };

            db.insert_latest_state(swap_id, new_state.clone().into())
                .await?;

            Ok(new_state)
        }
        _ => bail!(
            "Cannot grant final amnesty for swap {} because it is in state {} which is not BtcRefundBurnConfirmed",
            swap_id,
            state
        ),
    }
}
