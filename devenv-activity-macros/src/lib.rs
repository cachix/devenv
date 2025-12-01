//! Proc-macros for devenv activity instrumentation.
//!
//! This crate provides the `#[activity]` attribute macro for automatically
//! wrapping functions with Activity tracking.
//!
//! # Example
//!
//! ```ignore
//! use devenv_activity_macros::activity;
//!
//! #[activity("Building shell")]
//! async fn build_shell() -> Result<()> {
//!     // Function body is automatically instrumented with an Activity
//!     Ok(())
//! }
//! ```

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    spanned::Spanned,
    Expr, ExprLit, FnArg, Ident, ItemFn, Lit, Pat, Token,
};

/// Arguments for the `#[activity]` attribute.
///
/// Supports:
/// - `#[activity("name")]` - Simple operation activity
/// - `#[activity("name", kind = build)]` - Specify ActivityKind
/// - `#[activity("name", level = debug)]` - Specify tracing Level (trace, debug, info, warn, error)
/// - `#[activity("name", skip(arg1, arg2))]` - Skip certain arguments
struct ActivityArgs {
    name: Expr,
    kind: Option<Ident>,
    level: Option<Ident>,
    skip: Vec<Ident>,
}

impl Parse for ActivityArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut name: Option<Expr> = None;
        let mut kind: Option<Ident> = None;
        let mut level: Option<Ident> = None;
        let mut skip: Vec<Ident> = Vec::new();

        // Parse comma-separated arguments
        let args = Punctuated::<Expr, Token![,]>::parse_terminated(input)?;

        for (i, arg) in args.into_iter().enumerate() {
            match &arg {
                // First positional argument is the name
                Expr::Lit(ExprLit {
                    lit: Lit::Str(_), ..
                }) if i == 0 && name.is_none() => {
                    name = Some(arg);
                }
                // Handle key = value pairs
                Expr::Assign(assign) => {
                    let key = assign.left.to_token_stream().to_string();
                    match key.as_str() {
                        "kind" => {
                            if let Expr::Path(path) = &*assign.right {
                                if let Some(ident) = path.path.get_ident() {
                                    kind = Some(ident.clone());
                                }
                            }
                        }
                        "level" => {
                            if let Expr::Path(path) = &*assign.right {
                                if let Some(ident) = path.path.get_ident() {
                                    level = Some(ident.clone());
                                }
                            }
                        }
                        "skip" => {
                            // Parse skip(arg1, arg2)
                            if let Expr::Call(call) = &*assign.right {
                                for arg in &call.args {
                                    if let Expr::Path(path) = arg {
                                        if let Some(ident) = path.path.get_ident() {
                                            skip.push(ident.clone());
                                        }
                                    }
                                }
                            }
                        }
                        _ => {
                            return Err(syn::Error::new(
                                assign.span(),
                                format!("unknown attribute: {}", key),
                            ));
                        }
                    }
                }
                // Handle skip(arg1, arg2) without assignment
                Expr::Call(call) => {
                    if let Expr::Path(path) = &*call.func {
                        if path.path.is_ident("skip") {
                            for arg in &call.args {
                                if let Expr::Path(path) = arg {
                                    if let Some(ident) = path.path.get_ident() {
                                        skip.push(ident.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {
                    if name.is_none() {
                        name = Some(arg);
                    }
                }
            }
        }

        let name = name.ok_or_else(|| {
            syn::Error::new(input.span(), "activity name is required as first argument")
        })?;

        Ok(ActivityArgs { name, kind, level, skip })
    }
}

/// Attribute macro for instrumenting functions with Activity tracking.
///
/// # Usage
///
/// ```ignore
/// // Simple operation activity
/// #[activity("Building shell")]
/// async fn build_shell() -> Result<()> { ... }
///
/// // With specific kind
/// #[activity("Building container", kind = build)]
/// async fn build_container() -> Result<()> { ... }
///
/// // With specific level (trace, debug, info, warn, error)
/// #[activity("Running command", level = debug)]
/// async fn run_cmd() -> Result<()> { ... }
///
/// // Skip certain arguments (useful for &self)
/// #[activity("Running tests", skip(self))]
/// async fn run_tests(&self) -> Result<()> { ... }
///
/// // Dynamic name using format!
/// #[activity(format!("Building {} container", name))]
/// async fn build_named(&self, name: &str) -> Result<()> { ... }
/// ```
///
/// # Expansion
///
/// The macro expands to wrap the function body with Activity creation and instrumentation:
///
/// ```ignore
/// async fn build_shell() -> Result<()> {
///     let __activity = devenv_activity::Activity::operation("Building shell");
///     async move {
///         // original function body
///     }.instrument(__activity.span()).await
/// }
/// ```
#[proc_macro_attribute]
pub fn activity(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ActivityArgs);
    let input_fn = parse_macro_input!(input as ItemFn);

    match generate_activity_wrapper(args, input_fn) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn generate_activity_wrapper(args: ActivityArgs, input_fn: ItemFn) -> syn::Result<TokenStream2> {
    let ActivityArgs { name, kind, level, skip } = args;

    let fn_vis = &input_fn.vis;
    let fn_sig = &input_fn.sig;
    let fn_attrs = &input_fn.attrs;
    let fn_block = &input_fn.block;

    let is_async = fn_sig.asyncness.is_some();

    // Generate the kind enum value
    let kind_enum = match kind {
        Some(ref k) => {
            let kind_str = k.to_string();
            match kind_str.as_str() {
                "build" => quote! { devenv_activity::ActivityKind::Build },
                "fetch" => quote! { devenv_activity::ActivityKind::Fetch },
                "evaluate" => quote! { devenv_activity::ActivityKind::Evaluate },
                "task" => quote! { devenv_activity::ActivityKind::Task },
                "command" => quote! { devenv_activity::ActivityKind::Command },
                "operation" | _ => quote! { devenv_activity::ActivityKind::Operation },
            }
        }
        None => quote! { devenv_activity::ActivityKind::Operation },
    };

    // Generate the level enum value (default to INFO)
    let level_enum = match level {
        Some(ref l) => {
            let level_str = l.to_string().to_lowercase();
            match level_str.as_str() {
                "trace" => quote! { devenv_activity::ActivityLevel::Trace },
                "debug" => quote! { devenv_activity::ActivityLevel::Debug },
                "warn" => quote! { devenv_activity::ActivityLevel::Warn },
                "error" => quote! { devenv_activity::ActivityLevel::Error },
                "info" | _ => quote! { devenv_activity::ActivityLevel::Info },
            }
        }
        None => quote! { devenv_activity::ActivityLevel::Info },
    };

    // Generate the activity creation call using builder
    let activity_create = quote! {
        devenv_activity::Activity::builder(#kind_enum, #name)
            .level(#level_enum)
            .start()
    };

    // Collect argument names that aren't skipped (for potential future use)
    let _captured_args: Vec<_> = fn_sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let FnArg::Typed(pat_type) = arg {
                if let Pat::Ident(pat_ident) = &*pat_type.pat {
                    let ident = &pat_ident.ident;
                    if !skip.iter().any(|s| s == ident) {
                        return Some(ident.clone());
                    }
                }
            }
            None
        })
        .collect();

    let output = if is_async {
        // For async functions, wrap the body in an async block and instrument it
        quote! {
            #(#fn_attrs)*
            #fn_vis #fn_sig {
                let __activity = #activity_create;
                async move #fn_block.instrument(__activity.span()).await
            }
        }
    } else {
        // For sync functions, use in_scope
        quote! {
            #(#fn_attrs)*
            #fn_vis #fn_sig {
                let __activity = #activity_create;
                __activity.in_scope(|| #fn_block)
            }
        }
    };

    Ok(output)
}
