use proc_macro::TokenStream as TokenStream1;

#[proc_macro_derive(Message, attributes(reply))]
pub fn derive_message(item: TokenStream1) -> TokenStream1 {
    derive_message::derive_message(item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

mod derive_message {
    use proc_macro2::{Ident, TokenStream};
    use quote::quote;
    use syn::{parse::Parse, parse2, Attribute, Error, Generics, Item, Type};

    struct DeriveMessage {
        ident: Ident,
        attrs: Vec<Attribute>,
        generics: Generics,
    }

    impl Parse for DeriveMessage {
        fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
            match input.parse::<Item>()? {
                Item::Enum(item) => Ok(DeriveMessage {
                    ident: item.ident,
                    attrs: item.attrs,
                    generics: item.generics,
                }),
                Item::Struct(item) => Ok(DeriveMessage {
                    ident: item.ident,
                    attrs: item.attrs,
                    generics: item.generics,
                }),
                Item::Union(item) => Ok(DeriveMessage {
                    ident: item.ident,
                    attrs: item.attrs,
                    generics: item.generics,
                }),
                item => Err(Error::new_spanned(item, "Must be enum, struct or union")),
            }
        }
    }

    pub fn derive_message(item: TokenStream) -> Result<TokenStream, Error> {
        let DeriveMessage {
            ident,
            attrs,
            generics,
        } = parse2::<DeriveMessage>(item)?;
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        let returns = match attrs.iter().find(|attr| attr.path.is_ident("reply")) {
            Some(attr) => {
                let ty = attr.parse_args::<Type>()?;
                quote! { tiny_actor::Rx<#ty> }
            }
            None => quote! { () },
        };

        Ok(quote! {
            impl #impl_generics tiny_actor::Message for #ident #ty_generics #where_clause {
                type Returns = #returns;
            }
        })
    }
}

#[proc_macro_attribute]
pub fn protocol(attr: TokenStream1, item: TokenStream1) -> TokenStream1 {
    protocol::protocol(attr.into(), item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

mod protocol {
    use proc_macro2::{Ident, TokenStream, Span};
    use quote::quote;
    use syn::{parse2, Error, ItemEnum, Type, Fields, Field, Visibility, parse_quote, spanned::Spanned};

    pub fn protocol(_attr: TokenStream, item: TokenStream) -> Result<TokenStream, Error> {
        let mut item = parse2::<ItemEnum>(item)?;

        let variants = extend_enum(&mut item)?;
        let impl_protocol = impl_protocol(&item, &variants)?;
        let impl_accepts = impl_accepts(&item, &variants)?;

        Ok(quote! {
            #item
            #impl_protocol
            #impl_accepts
        })
    }

    struct ProtocolVariant {
        ident: Ident,
        ty: Type,
    }

    fn impl_accepts(item: &ItemEnum, variants: &Vec<ProtocolVariant>) -> Result<TokenStream, Error> {
        let ident = &item.ident;
        let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();

        let accepts = variants.iter().map(|variant| {
            let variant_ty = &variant.ty;
            let variant_ident = &variant.ident;
            quote! {
                impl #impl_generics tiny_actor::Accepts<#variant_ty> for #ident #ty_generics #where_clause {
                    fn from_msg(
                        msg: #variant_ty, 
                        tx: <<#variant_ty as tiny_actor::Message>::Returns as tiny_actor::Receiver>::Sender
                    ) -> Self {
                        Self::#variant_ident(msg, tx)
                    }
                }
            }
        }).collect::<Vec<_>>();

        Ok(quote! {
            #(#accepts)*
        })
    }

    fn impl_protocol(item: &ItemEnum, variants: &Vec<ProtocolVariant>) -> Result<TokenStream, Error> {
        let ident = &item.ident;
        let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();

        let downcasts = variants.iter().map(|variant| {
            let variant_ident = &variant.ident;
            quote! {
                let boxed = match boxed.downcast() {
                    Ok((msg, returns)) => {
                        return Ok(Self::#variant_ident(msg, returns));
                    }
                    Err(e) => e,
                };
            }
        }).collect::<Vec<_>>();

        let accepts = variants.iter().map(|variant| {
            let variant_ty = &variant.ty;
            quote! {
                if *msg_type_id == core::any::TypeId::of::<#variant_ty>() {
                    return true
                }
            }
        }).collect::<Vec<_>>();

        Ok(quote! {
            impl #impl_generics tiny_actor::Protocol for #ident #ty_generics #where_clause {

                fn try_from_msg(boxed: tiny_actor::BoxedMessage) -> Result<Self, tiny_actor::BoxedMessage> {
                    #(#downcasts)*
                    Err(boxed)
                }

                fn accepts(msg_type_id: &core::any::TypeId) -> bool {
                    #(#accepts)*
                    false
                }

            }
        })
    }

    fn extend_enum(item: &mut ItemEnum) -> Result<Vec<ProtocolVariant>, Error> {
        item.variants.iter_mut().map(|variant| {
            let ident = variant.ident.clone();

            if let Fields::Unnamed(fields) = &mut variant.fields {
                if fields.unnamed.len() == 1 {
                    let ty = fields.unnamed.iter().next().unwrap().ty.clone();

                    fields.unnamed.push(Field {
                        attrs: Vec::new(),
                        vis: Visibility::Inherited,
                        ident: None,
                        colon_token: None,
                        ty: parse_quote! { 
                            <<#ty as tiny_actor::Message>::Returns as tiny_actor::Receiver>::Sender 
                        },
                    });

                    Ok(ProtocolVariant {
                        ident,
                        ty
                    })
                } else {
                    Err(Error::new_spanned(fields, "Must have one field"))
                }
            } else {
                Err(Error::new(Span::call_site(), "Must be unnamed enum"))
            }
        }).collect()
    }
}
