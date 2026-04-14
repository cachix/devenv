//! Proc-macros for devenv activity instrumentation.
//!
//! This crate provides the `#[instrument_activity]` attribute macro for automatically
//! wrapping functions with Activity tracking.
//!
//! # Example
//!
//! ```ignore
//! use devenv_activity_macros::instrument_activity;
//!
//! #[instrument_activity("Building shell")]
//! async fn build_shell() -> Result<()> {
//!     // Function body is automatically instrumented with an Activity
//!     Ok(())
//! }
//! ```

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use syn::{
    Expr, ExprLit, Ident, ItemFn, Lit, Token,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    spanned::Spanned,
};

/// Arguments for the `#[activity]` attribute.
///
/// Supports:
/// - `#[instrument_activity("name")]` - Simple operation activity
/// - `#[instrument_activity("name", kind = build)]` - Specify activity type (build, evaluate, task, command, operation)
/// - `#[instrument_activity("name", level = debug)]` - Specify tracing Level (trace, debug, info, warn, error)
/// - `#[instrument_activity("name", skip(arg1, arg2))]` - Skip certain arguments
///
/// Note: `fetch` is not available as a kind since it requires a FetchKind parameter.
struct ActivityArgs {
    name: Expr,
    kind: Option<Ident>,
    level: Option<Ident>,
    #[allow(dead_code)] // Parsed for compatibility but not yet used for span field capture.
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
                            if let Expr::Path(path) = &*assign.right
                                && let Some(ident) = path.path.get_ident()
                            {
                                kind = Some(ident.clone());
                            }
                        }
                        "level" => {
                            if let Expr::Path(path) = &*assign.right
                                && let Some(ident) = path.path.get_ident()
                            {
                                level = Some(ident.clone());
                            }
                        }
                        "skip" => {
                            // Parse skip(arg1, arg2)
                            if let Expr::Call(call) = &*assign.right {
                                for arg in &call.args {
                                    if let Expr::Path(path) = arg
                                        && let Some(ident) = path.path.get_ident()
                                    {
                                        skip.push(ident.clone());
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
                    if let Expr::Path(path) = &*call.func
                        && path.path.is_ident("skip")
                    {
                        for arg in &call.args {
                            if let Expr::Path(path) = arg
                                && let Some(ident) = path.path.get_ident()
                            {
                                skip.push(ident.clone());
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

        Ok(ActivityArgs {
            name,
            kind,
            level,
            skip,
        })
    }
}

/// Attribute macro for instrumenting functions with Activity tracking.
///
/// # Usage
///
/// ```ignore
/// // Simple operation activity
/// #[instrument_activity("Building shell")]
/// async fn build_shell() -> Result<()> { ... }
///
/// // With specific kind (view adds "Building" prefix for build kind)
/// #[instrument_activity("container", kind = build)]
/// async fn build_container() -> Result<()> { ... }
///
/// // With specific level (trace, debug, info, warn, error)
/// #[instrument_activity("Running command", level = debug)]
/// async fn run_cmd() -> Result<()> { ... }
///
/// // Skip certain arguments (useful for &self)
/// #[instrument_activity("Running tests", skip(self))]
/// async fn run_tests(&self) -> Result<()> { ... }
///
/// // Dynamic name using format! (for build kind, omit verb - view adds it)
/// #[instrument_activity(format!("{} container", name), kind = build)]
/// async fn build_named(&self, name: &str) -> Result<()> { ... }
/// ```
///
/// # Expansion
///
/// The macro expands to wrap the function body with Activity creation and instrumentation:
///
/// ```ignore
/// async fn build_shell() -> Result<()> {
///     use devenv_activity::ActivityInstrument;
///     let __activity = devenv_activity::Activity::operation("Building shell").start();
///     (async move {
///         // original function body
///     }).in_activity(&__activity).await
/// }
/// ```
#[proc_macro_attribute]
pub fn instrument_activity(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ActivityArgs);
    let input_fn = parse_macro_input!(input as ItemFn);

    match generate_activity_wrapper(args, input_fn) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn generate_activity_wrapper(args: ActivityArgs, input_fn: ItemFn) -> syn::Result<TokenStream2> {
    let ActivityArgs {
        name,
        kind,
        level,
        skip: _,
    } = args;

    let fn_vis = &input_fn.vis;
    let fn_sig = &input_fn.sig;
    let fn_attrs = &input_fn.attrs;
    let fn_block = &input_fn.block;

    let is_async = fn_sig.asyncness.is_some();

    // Generate the level enum value (default to INFO)
    let level_str = level
        .as_ref()
        .map(|l| l.to_string().to_lowercase())
        .unwrap_or_else(|| "info".to_string());

    let level_enum = match level_str.as_str() {
        "trace" => quote! { devenv_activity::ActivityLevel::Trace },
        "debug" => quote! { devenv_activity::ActivityLevel::Debug },
        "info" => quote! { devenv_activity::ActivityLevel::Info },
        "warn" => quote! { devenv_activity::ActivityLevel::Warn },
        "error" => quote! { devenv_activity::ActivityLevel::Error },
        _ => {
            return Err(syn::Error::new(
                level.as_ref().unwrap().span(),
                format!(
                    "unknown level '{}', expected one of: trace, debug, info, warn, error",
                    level_str
                ),
            ));
        }
    };

    // Generate the builder expression. The activity!() macro handles span creation
    // at the expansion site so tracing metadata points to the annotated function.
    let activity_builder = match kind {
        Some(ref k) => {
            let kind_str = k.to_string();
            match kind_str.as_str() {
                "build" => quote! {
                    devenv_activity::Activity::build(#name).level(#level_enum)
                },
                "evaluate" => quote! {
                    devenv_activity::Activity::evaluate(#name).level(#level_enum)
                },
                "task" => quote! {
                    devenv_activity::Activity::task(#name).level(#level_enum)
                },
                "command" => quote! {
                    devenv_activity::Activity::command(#name).level(#level_enum)
                },
                "operation" => quote! {
                    devenv_activity::Activity::operation(#name).level(#level_enum)
                },
                _ => {
                    return Err(syn::Error::new(
                        k.span(),
                        format!(
                            "unknown kind '{}', expected one of: build, evaluate, task, command, operation",
                            kind_str
                        ),
                    ));
                }
            }
        }
        None => quote! {
            devenv_activity::Activity::operation(#name).level(#level_enum)
        },
    };

    let activity_create = quote! {
        devenv_activity::start!(#activity_builder)
    };

    let output = if is_async {
        // For async functions, use in_activity() which handles both parent tracking and span instrumentation
        quote! {
            #(#fn_attrs)*
            #fn_vis #fn_sig {
                use devenv_activity::ActivityInstrument;
                let __activity = #activity_create;
                (async move #fn_block).in_activity(&__activity).await
            }
        }
    } else {
        // For sync functions, use with_new_scope_sync() for parent tracking and in_scope() for span
        quote! {
            #(#fn_attrs)*
            #fn_vis #fn_sig {
                let __activity = #activity_create;
                __activity.with_new_scope_sync(|| #fn_block)
            }
        }
    };

    Ok(output)
}
