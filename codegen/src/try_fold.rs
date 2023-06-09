use heck::ToSnakeCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::str::FromStr;
use syn_codegen::{Data, Definitions, Node, Punctuated, Type};

type TS = TokenStream;

pub fn gen(defs: &Definitions) -> TS {
    let mut ts = gen_trait(&defs.types);
    ts.extend(gen_fns(&defs.types));
    ts
}

fn gen_trait(nodes: &Vec<Node>) -> TS {
    let mut ts = TS::new();
    for n in nodes {
        // node fn
        let n_ty = to_ident_ts(&n.ident);
        let n_fn = {
            let name = format_ident!("try_fold_{}", n.ident.to_snake_case());
            quote! {
                fn #name(&mut self, t: syn::#n_ty) -> Result<syn::#n_ty, Self::Error> {
                    #name(self, t)
                }
            }
        };
        ts.extend(n_fn);

        // node::enum
        if let Data::Enum(e) = &n.data {
            for (k, v) in e {
                if v.is_empty() {
                    continue;
                }
                let e_ty = types_ty(v);
                let e_fn = {
                    let name =
                        format_ident!("try_fold_{}_variant_{}", n.ident.to_snake_case(), k.to_snake_case());
                    quote! {
                        fn #name(&mut self, t: #e_ty) -> Result<syn::#n_ty, Self::Error> {
                            #name(self, t)
                        }
                    }
                };
                ts.extend(e_fn);
            }
        }
    }

    quote! {
        /// `fn try_fold_T(&mut self, T) -> Result<T, Self::Error>`
        ///
        /// It can fold syntax tree and report error
        pub trait TryFold {
            type Error;

            #ts
        }
    }
}

fn gen_fns(nodes: &Vec<Node>) -> TS {
    let mut ts = TS::new();

    for n in nodes {
        let n_ty = to_ident_ts(&n.ident);
        match &n.data {
            Data::Private => {
                let n_fn = {
                    let name = format_ident!("try_fold_{}", n.ident.to_snake_case());
                    quote! {
                        pub fn #name<F>(f: &mut F, t: syn::#n_ty) -> Result<syn::#n_ty, <F as TryFold>::Error>
                        where
                            F: TryFold + ?Sized,
                        {
                            Ok(t)
                        }
                    }
                };
                ts.extend(n_fn);
            }
            Data::Struct(fields) => {
                // node fn
                let n_fn = {
                    let name = format_ident!("try_fold_{}", n.ident.to_snake_case());
                    let fold = fields.iter().map(|(f, t)| {
                        let t = gen_fold(TS::from_str(&format!("t.{}", f)).unwrap(), t);
                        let f = to_ident_ts(f);
                        quote!(t.#f = #t)
                    });
                    quote! {
                        pub fn #name<F>(f: &mut F, t: syn::#n_ty) -> Result<syn::#n_ty, <F as TryFold>::Error>
                        where
                            F: TryFold + ?Sized,
                        {
                            let mut t = t;
                            #(#fold;)*
                            Ok(t)
                        }
                    }
                };
                ts.extend(n_fn);
            }
            Data::Enum(variants) => {
                // node fn
                let n_fn = {
                    let name = format_ident!("try_fold_{}", n.ident.to_snake_case());
                    let fold = variants.iter().filter_map(|(v, t)| {
                        if t.is_empty() {
                            return None;
                        }
                        let fold_name = format_ident!(
                            "try_fold_{}_variant_{}",
                            n.ident.to_snake_case(),
                            v.to_snake_case()
                        );
                        let v = to_ident_ts(v);
                        let unpack: Vec<_> =
                            (0..t.len()).map(|x| format_ident!("tmp{}", x)).collect();
                        Some(quote!(syn::#n_ty::#v(#(#unpack),*) => f.#fold_name((#(#unpack),*))?))
                    });


                    quote! {
                        pub fn #name<F>(f: &mut F, t: syn::#n_ty) -> Result<syn::#n_ty, <F as TryFold>::Error>
                        where
                            F: TryFold + ?Sized,
                        {
                            let t = match t {
                                #(#fold,)*
                                t => t,
                            };
                            Ok(t)
                        }
                    }
                };
                ts.extend(n_fn);

                // enum fn
                for (k, v) in variants {
                    if v.is_empty() {
                        continue;
                    }
                    let field = to_ident_ts(k);
                    let n_fn = {
                        let name = format_ident!(
                            "try_fold_{}_variant_{}",
                            n.ident.to_snake_case(),
                            k.to_snake_case()
                        );
                        let t = types_ty(v);
                        let fold = gen_folds(quote!(t), v);
                        let unpack = unpack(quote!(t), v.len());
                        quote! {
                            pub fn #name<F>(f: &mut F, t: #t) -> Result<syn::#n_ty, <F as TryFold>::Error>
                            where
                                F: TryFold + ?Sized,
                            {
                                let t = #fold;
                                Ok(syn::#n_ty::#field(#(#unpack),*))
                            }
                        }
                    };
                    ts.extend(n_fn);
                }
            }
        }
    }
    ts
}

fn gen_folds(field: TS, v: &Vec<Type>) -> TS {
    let v = unpack(field, v.len())
        .into_iter()
        .zip(v)
        .map(|(f, t)| gen_fold(f, t));
    quote! {(#(#v),*)}
}

fn gen_fold(field: TS, ty: &Type) -> TS {
    match ty {
        Type::Syn(s) => {
            if s == "Reserved" {
                field
            } else {
                let name = format_ident!("try_fold_{}", s.to_snake_case());
                quote! {
                    f.#name(#field)?
                }
            }
        }
        Type::Punctuated(Punctuated { element, .. }) => {
            let t = gen_fold(quote!(v), element);
            quote! {
                {
                    let mut tmp = syn::punctuated::Punctuated::new();
                    for v in #field {
                        tmp.push(#t);
                    }
                    tmp
                }
            }
        }
        Type::Vec(t) => {
            let t = gen_fold(quote!(v), t);
            quote! {
                {
                    let mut tmp = vec![];
                    for v in #field {
                        tmp.push(#t);
                    }
                    tmp
                }
            }
        }
        Type::Option(t) => {
            let t = gen_fold(quote!(o), t);
            quote! {
                match #field {
                    Some(o) => Some(#t),
                    None => None,
                }
            }
        }
        Type::Box(t) => {
            let t = gen_fold(quote!(*#field), t);
            quote!(Box::new(#t))
        }
        Type::Tuple(v) => gen_folds(field, v),
        _ => field,
    }
}

fn types_ty(v: &Vec<Type>) -> TS {
    let v = v.iter().map(type_ty);
    quote!((#(#v),*))
}

fn type_ty(ty: &Type) -> TS {
    match ty {
        Type::Syn(s) => {
            let s = to_ident_ts(s);
            quote!(syn::#s)
        }
        Type::Std(s) => {
            let s = to_ident_ts(s);
            quote!(#s)
        }
        Type::Ext(s) => {
            let s = to_ident_ts(s);
            quote!(proc_macro2::#s)
        }
        Type::Token(s) | Type::Group(s) => {
            let s = format_ident!("{}", s);
            quote!(syn::token::#s)
        }
        Type::Punctuated(p) => {
            let t = type_ty(&p.element);
            let p = format_ident!("{}", p.punct);
            quote!(syn::punctuated::Punctuated<#t, #p>)
        }
        Type::Option(t) => {
            let t = type_ty(t);
            quote!(Option<#t>)
        }
        Type::Box(t) => {
            let t = type_ty(t);
            quote!(Box<#t>)
        }
        Type::Vec(t) => {
            let t = type_ty(t);
            quote!(Vec<#t>)
        }
        Type::Tuple(v) => types_ty(v),
    }
}

// fn data_ty(nodes: &Vec<Node>) -> TS {}

fn to_ident_ts(s: &String) -> TS {
    let s = format_ident!("{}", s);
    quote!(#s)
}

fn unpack(f: TS, n: usize) -> Vec<TS> {
    if n == 1 {
        vec![f]
    } else {
        (0..n)
            .map(|x| {
                let x = TS::from_str(&x.to_string()).unwrap();
                quote!(#f.#x)
            })
            .collect()
    }
}
