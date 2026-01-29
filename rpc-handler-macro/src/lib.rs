extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, ItemFn};

#[proc_macro_attribute]
pub fn rpc_handler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let func_name = &func.sig.ident;
    let func_name_str = func_name.to_string();

    // Extract argument types
    let args_str = func.sig.inputs.iter()
        .filter_map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => Some(pat_type.ty.to_token_stream().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ");

    // Extract return type
    let return_type_str = match &func.sig.output {
        syn::ReturnType::Default => "()".to_string(),
        syn::ReturnType::Type(_, ty) => ty.to_token_stream().to_string(),
    };

    let expanded = quote! {
        inventory::submit! {
            crate::rpc::RpcHandler {
                name: #func_name_str,
                args: #args_str,
                return_type: #return_type_str,
            }
        }

        #[tauri::command]
        #func
    };

    expanded.into()
}
