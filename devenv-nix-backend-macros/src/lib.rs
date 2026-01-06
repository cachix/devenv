//! Proc-macros for devenv-nix-backend test instrumentation.
//!
//! Provides the `#[nix_test]` attribute macro for async tests that need
//! proper Boehm GC thread registration.
//!
//! # Example
//!
//! ```ignore
//! use devenv_nix_backend::nix_test;
//!
//! #[nix_test]
//! async fn test_something() {
//!     // Test body runs in a tokio runtime with GC-registered worker threads
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

/// Attribute macro for async tests that require Boehm GC thread registration.
///
/// This macro transforms an async test function into a synchronous test that:
/// 1. Initializes Nix/GC
/// 2. Creates a multi-threaded tokio runtime with GC-registered worker threads
/// 3. Runs the async test body in that runtime
///
/// # Usage
///
/// ```ignore
/// #[nix_test]
/// async fn test_backend_creation() {
///     // ...
/// }
///
/// #[nix_test]
/// #[ignore]
/// async fn test_slow_operation() {
///     // ...
/// }
/// ```
///
/// # Why This Is Needed
///
/// Nix uses Boehm GC with parallel marking. During stop-the-world collection,
/// only registered threads are paused. Unregistered tokio worker threads can
/// cause race conditions when parallel markers access memory those threads
/// are modifying.
#[proc_macro_attribute]
pub fn nix_test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);

    let fn_name = &input_fn.sig.ident;
    let fn_block = &input_fn.block;
    let fn_attrs = &input_fn.attrs;
    let fn_vis = &input_fn.vis;

    // Verify it's an async function
    if input_fn.sig.asyncness.is_none() {
        return syn::Error::new_spanned(&input_fn.sig, "nix_test requires an async function")
            .to_compile_error()
            .into();
    }

    // Generate the wrapped test
    let output = quote! {
        #(#fn_attrs)*
        #[test]
        #fn_vis fn #fn_name() {
            devenv_nix_backend::nix_init();
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .on_thread_start(|| {
                    let _ = devenv_nix_backend::gc_register_current_thread();
                })
                .build()
                .expect("Failed to create test runtime")
                .block_on(async #fn_block)
        }
    };

    output.into()
}
