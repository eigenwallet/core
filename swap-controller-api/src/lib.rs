use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObjectOwned;

#[rpc(client, server)]
pub trait AsbApi {
    #[method(name = "check_connection")]
    async fn check_connection(&self) -> Result<(), ErrorObjectOwned>;
}
