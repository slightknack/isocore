//! Derive macros for neopack's `Pack` and `Unpack` traits.
//!
//! Instead of writing serialization code by hand,
//! add `#[derive(Pack, Unpack)]` to your struct or enum.
//!
//! Structs encode as a list of their fields:
//!
//! ```ignore
//! #[derive(Pack, Unpack)]
//! struct Point { x: f64, y: f64 }
//! ```
//!
//! Newtypes (single-field tuple structs) encode as their inner value directly:
//!
//! ```ignore
//! #[derive(Pack, Unpack)]
//! struct UserId(u64);
//! ```
//!
//! Enums are tagged by variant name. Each variant can be unit, tuple, or struct:
//!
//! ```ignore
//! #[derive(Pack, Unpack)]
//! enum Shape {
//!     Empty,
//!     Circle(f64),
//!     Rect { w: f64, h: f64 },
//! }
//! ```
//!
//! Use `#[pack(bytes)]` on a `Vec<u8>` field to encode it as a
//! byte blob rather than a list of individual bytes:
//!
//! ```ignore
//! #[derive(Pack, Unpack)]
//! struct Message {
//!     topic: String,
//!     #[pack(bytes)]
//!     payload: Vec<u8>,
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::Data;
use syn::DeriveInput;
use syn::Fields;

#[proc_macro_derive(Pack, attributes(pack))]
pub fn derive_pack(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match derive_pack_impl(&input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[proc_macro_derive(Unpack, attributes(pack))]
pub fn derive_unpack(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match derive_unpack_impl(&input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn has_bytes_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("pack") { return false; }
        let mut found = false;
        let _ = a.parse_nested_meta(|meta| {
            if meta.path.is_ident("bytes") { found = true; }
            Ok(())
        });
        found
    })
}

// ── Pack ──

fn derive_pack_impl(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let (impl_g, ty_g, where_g) = input.generics.split_for_impl();

    let body = match &input.data {
        Data::Struct(s) => pack_struct(&s.fields)?,
        Data::Enum(e) => pack_enum(e)?,
        Data::Union(_) => return Err(syn::Error::new_spanned(name, "unions are not supported")),
    };

    Ok(quote! {
        impl #impl_g neopack::Pack for #name #ty_g #where_g {
            fn pack(&self, enc: &mut neopack::Encoder) -> neopack::Result<()> {
                #body
            }
        }
    })
}

fn pack_struct(fields: &Fields) -> syn::Result<proc_macro2::TokenStream> {
    match fields {
        Fields::Named(f) => {
            let stmts = f.named.iter().map(|field| {
                let ident = field.ident.as_ref().unwrap();
                pack_field_expr(quote!(self.#ident), &field.attrs)
            }).collect::<Vec<_>>();
            Ok(quote! {
                enc.list_begin()?;
                #(#stmts)*
                enc.list_end()
            })
        }
        Fields::Unnamed(f) if f.unnamed.len() == 1 => {
            let stmt = pack_field_expr(quote!(self.0), &f.unnamed[0].attrs);
            Ok(quote! { #stmt Ok(()) })
        }
        Fields::Unnamed(f) => {
            let stmts = f.unnamed.iter().enumerate().map(|(i, field)| {
                let idx = syn::Index::from(i);
                pack_field_expr(quote!(self.#idx), &field.attrs)
            }).collect::<Vec<_>>();
            Ok(quote! {
                enc.list_begin()?;
                #(#stmts)*
                enc.list_end()
            })
        }
        Fields::Unit => {
            Ok(quote! { enc.unit() })
        }
    }
}

fn pack_enum(e: &syn::DataEnum) -> syn::Result<proc_macro2::TokenStream> {
    let arms = e.variants.iter().map(|v| {
        let vname = &v.ident;
        let vstr = vname.to_string();
        match &v.fields {
            Fields::Unit => {
                quote! {
                    Self::#vname => {
                        enc.variant_begin(#vstr)?;
                        enc.unit()?;
                        enc.variant_end()
                    }
                }
            }
            Fields::Unnamed(f) if f.unnamed.len() == 1 => {
                let stmt = pack_field_expr(quote!(v0), &f.unnamed[0].attrs);
                quote! {
                    Self::#vname(v0) => {
                        enc.variant_begin(#vstr)?;
                        #stmt
                        enc.variant_end()
                    }
                }
            }
            Fields::Unnamed(f) => {
                let bindings: Vec<_> = (0..f.unnamed.len())
                    .map(|i| syn::Ident::new(&format!("v{i}"), proc_macro2::Span::call_site()))
                    .collect();
                let stmts: Vec<_> = f.unnamed.iter().enumerate().map(|(i, field)| {
                    let b = &bindings[i];
                    pack_field_expr(quote!(#b), &field.attrs)
                }).collect();
                quote! {
                    Self::#vname(#(#bindings),*) => {
                        enc.variant_begin(#vstr)?;
                        enc.list_begin()?;
                        #(#stmts)*
                        enc.list_end()?;
                        enc.variant_end()
                    }
                }
            }
            Fields::Named(f) => {
                let field_idents: Vec<_> = f.named.iter()
                    .map(|field| field.ident.as_ref().unwrap())
                    .collect();
                let stmts: Vec<_> = f.named.iter().map(|field| {
                    let ident = field.ident.as_ref().unwrap();
                    pack_field_expr(quote!(#ident), &field.attrs)
                }).collect();
                quote! {
                    Self::#vname { #(#field_idents),* } => {
                        enc.variant_begin(#vstr)?;
                        enc.list_begin()?;
                        #(#stmts)*
                        enc.list_end()?;
                        enc.variant_end()
                    }
                }
            }
        }
    }).collect::<Vec<_>>();

    Ok(quote! {
        match self {
            #(#arms)*
        }
    })
}

fn pack_field_expr(expr: proc_macro2::TokenStream, attrs: &[syn::Attribute]) -> proc_macro2::TokenStream {
    if has_bytes_attr(attrs) {
        quote! { enc.bytes(&#expr)?; }
    } else {
        quote! { neopack::Pack::pack(&#expr, enc)?; }
    }
}

// ── Unpack ──

fn derive_unpack_impl(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let (impl_g, ty_g, where_g) = input.generics.split_for_impl();

    let body = match &input.data {
        Data::Struct(s) => unpack_struct(name, &s.fields)?,
        Data::Enum(e) => unpack_enum(name, e)?,
        Data::Union(_) => return Err(syn::Error::new_spanned(name, "unions are not supported")),
    };

    Ok(quote! {
        impl #impl_g neopack::Unpack for #name #ty_g #where_g {
            fn unpack(dec: &mut neopack::Decoder<'_>) -> neopack::Result<Self> {
                #body
            }
        }
    })
}

fn unpack_struct(name: &syn::Ident, fields: &Fields) -> syn::Result<proc_macro2::TokenStream> {
    match fields {
        Fields::Named(f) => {
            let field_decodes: Vec<_> = f.named.iter().map(|field| {
                let ident = field.ident.as_ref().unwrap();
                let ty = &field.ty;
                let decode = unpack_field_expr(ty, &field.attrs);
                quote! { let #ident = { let mut d = list.next().ok_or(neopack::Error::UnexpectedEnd)?; #decode }; }
            }).collect();
            let field_names: Vec<_> = f.named.iter()
                .map(|field| field.ident.as_ref().unwrap())
                .collect();
            Ok(quote! {
                let mut list = dec.list()?;
                #(#field_decodes)*
                Ok(#name { #(#field_names),* })
            })
        }
        Fields::Unnamed(f) if f.unnamed.len() == 1 => {
            let ty = &f.unnamed[0].ty;
            let decode = unpack_field_expr(ty, &f.unnamed[0].attrs);
            Ok(quote! {
                let v0 = { let mut d = &mut *dec; #decode };
                Ok(#name(v0))
            })
        }
        Fields::Unnamed(f) => {
            let field_decodes: Vec<_> = f.unnamed.iter().enumerate().map(|(i, field)| {
                let var = syn::Ident::new(&format!("v{i}"), proc_macro2::Span::call_site());
                let ty = &field.ty;
                let decode = unpack_field_expr(ty, &field.attrs);
                quote! { let #var = { let mut d = list.next().ok_or(neopack::Error::UnexpectedEnd)?; #decode }; }
            }).collect();
            let vars: Vec<_> = (0..f.unnamed.len())
                .map(|i| syn::Ident::new(&format!("v{i}"), proc_macro2::Span::call_site()))
                .collect();
            Ok(quote! {
                let mut list = dec.list()?;
                #(#field_decodes)*
                Ok(#name(#(#vars),*))
            })
        }
        Fields::Unit => {
            Ok(quote! { dec.unit()?; Ok(#name) })
        }
    }
}

fn unpack_enum(name: &syn::Ident, e: &syn::DataEnum) -> syn::Result<proc_macro2::TokenStream> {
    let arms = e.variants.iter().map(|v| {
        let vname = &v.ident;
        let vstr = vname.to_string();
        match &v.fields {
            Fields::Unit => {
                quote! { #vstr => { inner.unit()?; Ok(#name::#vname) } }
            }
            Fields::Unnamed(f) if f.unnamed.len() == 1 => {
                let ty = &f.unnamed[0].ty;
                let decode = unpack_field_expr(ty, &f.unnamed[0].attrs);
                quote! { #vstr => { let mut d = &mut inner; let v0 = #decode; Ok(#name::#vname(v0)) } }
            }
            Fields::Unnamed(f) => {
                let field_decodes: Vec<_> = f.unnamed.iter().enumerate().map(|(i, field)| {
                    let var = syn::Ident::new(&format!("v{i}"), proc_macro2::Span::call_site());
                    let ty = &field.ty;
                    let decode = unpack_field_expr(ty, &field.attrs);
                    quote! { let #var = { let mut d = list.next().ok_or(neopack::Error::UnexpectedEnd)?; #decode }; }
                }).collect();
                let vars: Vec<_> = (0..f.unnamed.len())
                    .map(|i| syn::Ident::new(&format!("v{i}"), proc_macro2::Span::call_site()))
                    .collect();
                quote! {
                    #vstr => {
                        let mut list = inner.list()?;
                        #(#field_decodes)*
                        Ok(#name::#vname(#(#vars),*))
                    }
                }
            }
            Fields::Named(f) => {
                let field_decodes: Vec<_> = f.named.iter().map(|field| {
                    let ident = field.ident.as_ref().unwrap();
                    let ty = &field.ty;
                    let decode = unpack_field_expr(ty, &field.attrs);
                    quote! { let #ident = { let mut d = list.next().ok_or(neopack::Error::UnexpectedEnd)?; #decode }; }
                }).collect();
                let field_names: Vec<_> = f.named.iter()
                    .map(|field| field.ident.as_ref().unwrap())
                    .collect();
                quote! {
                    #vstr => {
                        let mut list = inner.list()?;
                        #(#field_decodes)*
                        Ok(#name::#vname { #(#field_names),* })
                    }
                }
            }
        }
    }).collect::<Vec<_>>();

    Ok(quote! {
        let (variant_name, mut inner) = dec.variant()?;
        match variant_name {
            #(#arms)*
            _ => Err(neopack::Error::InvalidTag(0)),
        }
    })
}

fn unpack_field_expr(ty: &syn::Type, attrs: &[syn::Attribute]) -> proc_macro2::TokenStream {
    if has_bytes_attr(attrs) {
        quote! { d.bytes()?.to_vec() }
    } else {
        quote! { <#ty as neopack::Unpack>::unpack(&mut d)? }
    }
}
