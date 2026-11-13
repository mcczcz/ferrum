//! Ferrum SDK 过程宏。请通过 `ferrum_sdk::mod_entry` 使用，不要直接依赖本 crate。

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Error, ItemImpl, LitStr, Meta, Token, Type, parse_macro_input};

struct EntryArgs {
    hot_unload_safe: bool,
    min_abi: Option<(u16, u16)>,
}

impl Parse for EntryArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = EntryArgs {
            hot_unload_safe: false,
            min_abi: None,
        };
        for meta in Punctuated::<Meta, Token![,]>::parse_terminated(input)? {
            match &meta {
                Meta::Path(p) if p.is_ident("hot_unload_safe") => args.hot_unload_safe = true,
                Meta::NameValue(nv) if nv.path.is_ident("min_abi") => {
                    let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = &nv.value
                    else {
                        return Err(Error::new_spanned(
                            &nv.value,
                            "min_abi 需要字符串，例如 \"1.3\"",
                        ));
                    };
                    args.min_abi = Some(parse_version(s)?);
                }
                _ => {
                    return Err(Error::new_spanned(
                        meta,
                        "未知参数；支持 `hot_unload_safe` 与 `min_abi = \"1.x\"`",
                    ));
                }
            }
        }
        Ok(args)
    }
}

fn parse_version(s: &LitStr) -> syn::Result<(u16, u16)> {
    let value = s.value();
    let mut parts = value.split('.');
    let parsed = (|| {
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        parts.next().is_none().then_some((major, minor))
    })();
    parsed.ok_or_else(|| Error::new(s.span(), "min_abi 格式应为 \"主版本.次版本\"，例如 \"1.3\""))
}

/// 把模组类型注册为 Ferrum 模组入口。
///
/// 标注在 `impl FerrumMod for MyMod { ... }` 上，展开后导出 `ferrum_mod_entry_v1`，
/// 完成 ABI 握手，并为全部生命周期回调装上 `catch_unwind` 防火墙。
///
/// 参数：
/// - `hot_unload_safe`：声明模组可被 `FreeLibrary`（见线程规范 §4.4）；
/// - `min_abi = "1.x"`：在加载阶段就要求宿主 ABI 至少为该版本。
#[proc_macro_attribute]
pub fn mod_entry(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as EntryArgs);
    let item_impl = parse_macro_input!(item as ItemImpl);

    let self_ty: &Type = &item_impl.self_ty;
    if item_impl.trait_.is_none() {
        return Error::new_spanned(
            &item_impl,
            "#[mod_entry] 必须标注在 `impl FerrumMod for YourMod { ... }` 上",
        )
        .to_compile_error()
        .into();
    }
    if !item_impl.generics.params.is_empty() {
        return Error::new_spanned(&item_impl.generics, "#[mod_entry] 不支持泛型模组类型")
            .to_compile_error()
            .into();
    }

    let flags = if args.hot_unload_safe {
        quote!(::ferrum_sdk::__private::abi::FerrumModFlags::HOT_UNLOAD_SAFE)
    } else {
        quote!(::ferrum_sdk::__private::abi::FerrumModFlags::NONE)
    };
    let min_abi = match args.min_abi {
        Some((major, minor)) => quote!(::core::option::Option::Some((#major, #minor))),
        None => quote!(::core::option::Option::None),
    };
    let entry_ident = syn::Ident::new("ferrum_mod_entry_v1", Span::call_site());

    quote! {
        #item_impl

        const _: () = {
            static __FERRUM_STATE: ::ferrum_sdk::__private::ModState<#self_ty> =
                ::ferrum_sdk::__private::ModState::new();

            extern "C" fn __ferrum_on_load(
                ctx: ::ferrum_sdk::__private::abi::ModContextHandle,
            ) -> ::ferrum_sdk::__private::abi::FerrumStatus {
                ::ferrum_sdk::__private::on_load::<#self_ty>(&__FERRUM_STATE, ctx)
            }

            extern "C" fn __ferrum_on_enable(
                ctx: ::ferrum_sdk::__private::abi::ModContextHandle,
                token: ::ferrum_sdk::__private::abi::RawMainThreadToken,
            ) -> ::ferrum_sdk::__private::abi::FerrumStatus {
                ::ferrum_sdk::__private::on_enable::<#self_ty>(&__FERRUM_STATE, ctx, token)
            }

            extern "C" fn __ferrum_on_disable(
                ctx: ::ferrum_sdk::__private::abi::ModContextHandle,
                token: ::ferrum_sdk::__private::abi::RawMainThreadToken,
            ) -> ::ferrum_sdk::__private::abi::FerrumStatus {
                ::ferrum_sdk::__private::on_disable::<#self_ty>(&__FERRUM_STATE, ctx, token)
            }

            extern "C" fn __ferrum_on_unload(
                ctx: ::ferrum_sdk::__private::abi::ModContextHandle,
            ) -> ::ferrum_sdk::__private::abi::FerrumStatus {
                ::ferrum_sdk::__private::on_unload::<#self_ty>(&__FERRUM_STATE, ctx)
            }

            /// Ferrum 模组唯一导出入口。
            ///
            /// # Safety
            /// 只能由 ferrum-loader 按 `ModEntryFnV1` 契约调用。
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn #entry_ident(
                host_api: *const ::ferrum_sdk::__private::abi::FerrumHostApi,
                ctx: ::ferrum_sdk::__private::abi::ModContextHandle,
                out_descriptor: *mut ::ferrum_sdk::__private::abi::FerrumModDescriptor,
            ) -> ::ferrum_sdk::__private::abi::FerrumStatus {
                let callbacks = ::ferrum_sdk::__private::EntryCallbacks {
                    flags: #flags,
                    min_abi: #min_abi,
                    on_load: __ferrum_on_load,
                    on_enable: __ferrum_on_enable,
                    on_disable: __ferrum_on_disable,
                    on_unload: __ferrum_on_unload,
                };
                // SAFETY: 透传宿主对 host_api 与 out_descriptor 的 ABI 保证。
                unsafe { ::ferrum_sdk::__private::entry(host_api, ctx, out_descriptor, callbacks) }
            }

            impl ::ferrum_sdk::ModEntryPoint for #self_ty {
                const ENTRY: ::ferrum_sdk::__private::abi::ModEntryFnV1 = #entry_ident;
            }
        };
    }
    .into()
}
