use inventory;

pub struct RpcHandler {
    pub name: &'static str,
    pub args: &'static str,
    pub return_type: &'static str,
}

inventory::collect!(RpcHandler);
