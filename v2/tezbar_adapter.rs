//! Compiled into native_core/extensions/v2_count_numbers.rs; see README integration.
use super::v2_sdk;
use crate::native_core::common::{CoreContext, CoreResult};
use serde_json::Value;
use tezbar_count_numbers_v2::{Host, HostFuture, StoreChange};

const SLUG: &str = "count-numbers";
struct TezbarHost<'a>(&'a CoreContext);
impl Host for TezbarHost<'_> {
    fn read_store(&self) -> HostFuture<'_> {
        Box::pin(v2_sdk::read_store(self.0, SLUG))
    }
    fn update_store(&self, change: StoreChange) -> HostFuture<'_> {
        Box::pin(v2_sdk::update_store(self.0, SLUG, change))
    }
}
pub fn commands() -> &'static [&'static str] {
    tezbar_count_numbers_v2::commands()
}
pub const ICON: &[u8] = tezbar_count_numbers_v2::ICON;
pub const MANIFEST: &str = tezbar_count_numbers_v2::MANIFEST;
pub async fn handle(
    ctx: &CoreContext,
    command: &str,
    operation: &str,
    payload: &Value,
    state: &mut Value,
) -> CoreResult {
    tezbar_count_numbers_v2::handle(&TezbarHost(ctx), command, operation, payload, state).await
}
