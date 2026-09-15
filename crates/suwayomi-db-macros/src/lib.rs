//! `#[derive(FromRow)]` for [`suwayomi_db::FromRow`].
//!
//! Mirrors the sqlx derive we replaced: every named field is read by its own
//! name, so the struct may declare its fields in any order and the SELECT may
//! return extra columns.
//!
//! Field→column mapping can be overridden with `#[db(rename = "...")]`; there
//! are no such overrides in the tree today, but the attribute keeps the door
//! open without another derive.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitStr, parse_macro_input};

/// Column names that `FromRow` should read, in field order.
fn column_names(fields: &syn::Fields) -> Vec<(syn::Ident, String)> {
    fields
        .iter()
        .map(|field| {
            let ident = field
                .ident
                .clone()
                .expect("FromRow requires named fields (tuples have hand-written impls)");
            let renamed = field.attrs.iter().find_map(|attr| {
                if !attr.path().is_ident("db") {
                    return None;
                }
                let mut found = None;
                let _ = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rename") {
                        let value: LitStr = meta.value()?.parse()?;
                        found = Some(value.value());
                    }
                    Ok(())
                });
                found
            });
            let column = renamed.unwrap_or_else(|| ident.to_string());
            (ident, column)
        })
        .collect()
}

/// Derives `suwayomi_db::FromRow` (sqlx-compatible column-name mapping).
#[proc_macro_derive(FromRow, attributes(db))]
pub fn derive_from_row(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let syn::Data::Struct(data) = &input.data else {
        return syn::Error::new_spanned(name, "FromRow can only be derived for structs")
            .to_compile_error()
            .into();
    };
    let init = column_names(&data.fields).into_iter().map(|(ident, column)| {
        quote! { #ident: ::suwayomi_db::Row::try_get(row, #column)? }
    });
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        impl #impl_generics ::suwayomi_db::FromRow for #name #ty_generics #where_clause {
            fn from_row(row: &::suwayomi_db::Row) -> ::suwayomi_db::Result<Self> {
                Ok(Self { #(#init),* })
            }
        }
    }
    .into()
}
