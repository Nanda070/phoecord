//! `#[phoecord::plugin]`: marks the one type of a guest crate that is the plugin. Used through
//! `phoecord-plugin-sdk`; no dependencies.

use proc_macro::{TokenStream, TokenTree};

/// Put it on the `impl Plugin for MyPlugin` block. It keeps the block and exports `MyPlugin` as the
/// component's plugin. The SDK must be the dependency named `phoecord`:
/// `phoecord = { package = "phoecord-plugin-sdk", ... }`.
#[proc_macro_attribute]
pub fn plugin(attr: TokenStream, item: TokenStream) -> TokenStream {
    let code = match (attr.is_empty(), plugin_type(item.clone())) {
        (true, Some(name)) => format!("::phoecord::__export!({name});"),
        _ => "::core::compile_error!(\"use #[phoecord::plugin] without arguments on \
              `impl Plugin for YourType`\");"
            .to_string(),
    };
    let mut out = item;
    out.extend(code.parse::<TokenStream>().expect("generated code parses"));
    out
}

/// `T` of `impl ... for T { ... }`: the single identifier between `for` and the block.
fn plugin_type(item: TokenStream) -> Option<String> {
    let mut tokens = item.into_iter();
    tokens.find(|t| matches!(t, TokenTree::Ident(i) if i.to_string() == "impl"))?;
    tokens.find(|t| matches!(t, TokenTree::Ident(i) if i.to_string() == "for"))?;
    let TokenTree::Ident(name) = tokens.next()? else {
        return None;
    };
    matches!(tokens.next()?, TokenTree::Group(_)).then(|| name.to_string())
}
