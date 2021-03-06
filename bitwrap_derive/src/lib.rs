extern crate proc_macro;

use proc_macro2::{
    TokenTree,
    TokenStream,
    Ident,
    token_stream::IntoIter,
};

use quote::quote;

use syn::{
    self,
    parse_macro_input,
};


struct BitWrapMacro {
    struct_id: Ident,
    pack_list: TokenStream,
    unpack_list: TokenStream,
    bits: usize,
}


// convert TokenTree literal to usize
fn literal_to_usize(item: &TokenTree) -> usize {
    if let TokenTree::Literal(v) = item {
        syn::LitInt::from(v.clone()).base10_parse::<usize>().unwrap_or(0)
    } else {
        0
    }
}


// push attribute option tokens to TokenStream
fn extend_token_stream(stream: &mut TokenStream, iter: &mut IntoIter)
{
    while let Some(item) = iter.next() {
        match item {
            TokenTree::Punct(v) if v.as_char() == ',' => break,
            v => stream.extend(quote! { #v }),
        }
    }
}


impl BitWrapMacro {
    fn new(ident: &Ident) -> Self {
        Self {
            struct_id: ident.clone(),
            pack_list: TokenStream::default(),
            unpack_list: TokenStream::default(),
            bits: 0,
        }
    }

    fn assert_align(&self) {
        assert_eq!(self.bits, 8, "bitwrap not aligned");
    }

    fn macro_make_bits(&mut self, ty: &Ident, bits: usize) {
        let mut bits = bits;

        self.unpack_list.extend(quote! {
            let mut value: #ty = 0;
        });

        while bits > self.bits {
            let shift = bits - self.bits; // value left shift
            let mask = 0xFFu8 >> (8 - self.bits);

            self.pack_list.extend(quote! {
                dst[offset] |= ((value >> #shift) as u8) & #mask;
                offset += 1;
                dst[offset] = 0;
            });

            self.unpack_list.extend(quote! {
                value |= ((src[offset] & #mask) as #ty) << #shift;
                offset += 1;
            });

            bits -= self.bits;
            self.bits = 8;
        }

        self.bits -= bits;

        let shift = self.bits; // byte right shift
        let mask = 0xFFu8 >> (8 - bits);

        if shift == 0 {
            self.pack_list.extend(quote! {
                dst[offset] |= (value as u8) & #mask;
                offset += 1;
            });

            self.unpack_list.extend(quote! {
                value |= (src[offset] & #mask) as #ty;
                offset += 1;
            });

            self.bits = 8;
        } else {
            self.pack_list.extend(quote! {
                dst[offset] |= ((value as u8) & #mask) << #shift;
            });

            self.unpack_list.extend(quote! {
                value |= ((src[offset] >> #shift) & #mask) as #ty;
            });
        }
    }

    fn macro_make_skip(&mut self, bits: usize, value: usize) {
        let mut bits = bits;

        while bits > self.bits {
            let shift = bits - self.bits; // value left shift
            let mask = 0xFFu8 >> (8 - self.bits);
            let v = ((value >> shift) as u8) & mask;

            self.pack_list.extend(quote! {
                dst[offset] |= #v;
                offset += 1;
                dst[offset] = 0;
            });

            self.unpack_list.extend(quote! {
                offset += 1;
            });

            bits -= self.bits;
            self.bits = 8;
        }

        self.bits -= bits;

        let shift = self.bits; // byte right shift
        let mask = 0xFFu8 >> (8 - bits);
        let v = ((value as u8) & mask) << shift;

        self.pack_list.extend(quote! {
            dst[offset] |= #v;
        });

        if shift == 0 {
            self.pack_list.extend(quote! {
                offset += 1;
            });

            self.unpack_list.extend(quote! {
                offset += 1;
            });

            self.bits = 8;
        }
    }

    fn build_bits(&mut self, field: &syn::Field, tokens: &TokenStream) {
        let field_ty = &field.ty;
        let field_ident = &field.ident;

        // Nested
        if tokens.is_empty() {
            self.assert_align();

            self.pack_list.extend(quote! {
                offset += self.#field_ident.pack(&mut dst[offset ..])?;
            });

            self.unpack_list.extend(quote! {
                offset += self.#field_ident.unpack(&src[offset ..])?;
            });

            return;
        }

        let tokens = tokens.clone();
        let tree = tokens.into_iter().next().unwrap();
        let group = match tree {
            TokenTree::Group(v) => v.stream(),
            _ => unreachable!(),
        };

        let mut iter = group.into_iter();

        let mut bits = 0;

        let mut field_name = TokenStream::new();
        let mut field_value = TokenStream::new();

        let mut convert_from = TokenStream::new();
        let mut convert_into = TokenStream::new();

        let mut skip_value: Option<usize> = None;

        // get bits
        if let Some(item) = iter.next() {
            bits = literal_to_usize(&item);

            if bits == 0 || bits > 64 {
                panic!("bits argument #1 should be a number in range 1 .. 64");
            }
        }

        // check buffer len
        if self.bits == 8 {
            let bytes = (bits + 7) / 8;

            self.pack_list.extend(quote! {
                if #bytes + offset > dst.len() {
                    return Err(bitwrap::BitWrapError);
                }

                dst[offset] = 0;
            });

            self.unpack_list.extend(quote! {
                if #bytes + offset > src.len() {
                    return Err(bitwrap::BitWrapError);
                }
            });
        }

        // get type to store bits
        let convert_ty = if bits <= 8 {
            "u8"
        } else if bits <= 16 {
            "u16"
        } else if bits <= 32 {
            "u32"
        } else {
            "u64"
        };
        let ty = Ident::new(convert_ty, proc_macro2::Span::call_site());

        // parse attributes
        while let Some(item) = iter.next() {
            match item {
                TokenTree::Punct(v) if v.as_char() == ',' => continue,
                TokenTree::Ident(v) => {
                    // skip '=' token after ident in attribute options
                    match iter.next() {
                        Some(TokenTree::Punct(v)) if v.as_char() == '=' => {}
                        _ => panic!("unexpected token")
                    }

                    match v.to_string().as_str() {
                        "skip" => {
                            if let Some(value) = iter.next() {
                                skip_value = Some(literal_to_usize(&value));
                            }
                        }

                        "from" => {
                            extend_token_stream(&mut convert_from, &mut iter);
                            convert_from.extend(quote! {
                                (value)
                            });
                        }
                        "into" => {
                            extend_token_stream(&mut convert_into, &mut iter);
                            convert_into.extend(quote! {
                                ( self.#field_ident )
                            });
                        }

                        "name" => {
                            extend_token_stream(&mut field_name, &mut iter);
                        }
                        "value" => {
                            extend_token_stream(&mut field_value, &mut iter);
                        }

                        v => panic!("bits has unexpected argument: {}", v),
                    }
                }
                _ => panic!("bits has wrong format"),
            }
        }

        // skip bits
        if let Some(value) = skip_value {
            self.macro_make_skip(bits, value);
            return;
        }

        if ! field_name.is_empty() {
            //  name + value

            self.pack_list.extend(quote! {
                let value = ( #field_value ) as #ty ;
                let #field_name = value ;
            });

            self.macro_make_bits(&ty, bits);

            self.unpack_list.extend(quote! {
                let #field_name = value ;
            });

            return;
        }

        // set default conversion field -> bits
        if convert_into.is_empty() {
            match field_ty {
                syn::Type::Path(v) if v.path.is_ident("bool") => {
                    convert_into.extend(quote! {
                        if self.#field_ident { 1 } else { 0 }
                    });
                }
                _ => {
                    convert_into.extend(quote! {
                        self.#field_ident as #ty
                    })
                }
            }
        }

        self.pack_list.extend(quote! {
            let value: #ty = #convert_into ;
        });

        self.macro_make_bits(&ty, bits);

        // set default conversion bits -> field
        if convert_from.is_empty() {
            match field_ty {
                syn::Type::Path(v) if v.path.is_ident("bool") => {
                    convert_from.extend(quote! {
                        value != 0
                    });
                }
                _ => {
                    convert_from.extend(quote! {
                        value as #field_ty
                    })
                }
            }
        }

        self.unpack_list.extend(quote! {
            self.#field_ident = #convert_from ;
        });
    }

    fn build_bytes(&mut self, field: &syn::Field, tokens: &TokenStream) {
        self.assert_align();

        let field_ty = &field.ty;
        let field_ident = &field.ident;

        if tokens.is_empty() {
            if let syn::Type::Array(_) = field_ty {
                // [u8; N]
                // TODO: replace with const generic. issue #3
                self.pack_list.extend(quote! {
                    let next = offset + self.#field_ident.len();

                    if dst.len() >= next {
                        dst[offset .. next].clone_from_slice(&self.#field_ident);
                        offset = next;
                    } else {
                        return Err(bitwrap::BitWrapError);
                    }
                });

                self.unpack_list.extend(quote! {
                    let next = offset + self.#field_ident.len();
                    if src.len() >= next {
                        self.#field_ident.clone_from_slice(&src[offset .. next]);
                        offset = next;
                    } else {
                        return Err(bitwrap::BitWrapError);
                    }
                });

                return;
            }

            self.pack_list.extend(quote! {
                offset += self.#field_ident.pack(&mut dst[offset ..])?;
            });

            self.unpack_list.extend(quote! {
                offset += self.#field_ident.unpack(&src[offset ..])?;
            });

            return;
        }

        let tokens = tokens.clone();
        let tree = tokens.into_iter().next().unwrap();
        let group = match tree {
            TokenTree::Group(v) => v.stream(),
            _ => unreachable!(),
        };

        let mut iter = group.into_iter();

        let mut bytes = TokenStream::new();
        extend_token_stream(&mut bytes, &mut iter);

        self.pack_list.extend(quote! {
            let limit = offset + ( #bytes ) as usize;
            if dst.len() >= limit {
                offset += self.#field_ident.pack(&mut dst[offset .. limit])?;
            } else {
                return Err(bitwrap::BitWrapError);
            }
        });

        self.unpack_list.extend(quote! {
            let limit = offset + ( #bytes ) as usize;
            if src.len() >= limit {
                offset += self.#field_ident.unpack(&src[offset .. limit])?;
            } else {
                return Err(bitwrap::BitWrapError);
            }
        });
    }

    fn build_field(&mut self, field: &syn::Field) {
        for attr in field.attrs.iter().filter(|v| v.path.segments.len() == 1) {
            match attr.path.segments[0].ident.to_string().as_str() {
                "bits" => self.build_bits(field, &attr.tokens),
                "bytes" => self.build_bytes(field, &attr.tokens),
                _ => {}
            };
        }
    }

    fn build(&mut self, data: &syn::DataStruct) -> TokenStream {
        self.bits = 8;

        let fields = match &data.fields {
            syn::Fields::Named(v) => &v.named,
            syn::Fields::Unnamed(_v) => unimplemented!(),
            syn::Fields::Unit => unimplemented!(),
        };

        for field in fields {
            self.build_field(field);
        }

        self.assert_align();

        let struct_id = &self.struct_id;
        let pack_list = &self.pack_list;
        let unpack_list = &self.unpack_list;

        quote! {
            impl bitwrap::BitWrap for #struct_id {
                fn pack(&self, dst: &mut [u8]) -> Result<usize, bitwrap::BitWrapError> {
                    let mut offset: usize = 0;
                    #pack_list
                    Ok(offset)
                }

                fn unpack(&mut self, src: &[u8]) -> Result<usize, bitwrap::BitWrapError> {
                    let mut offset: usize = 0;
                    #unpack_list
                    Ok(offset)
                }
            }
        }
    }
}


#[proc_macro_derive(BitWrap, attributes(bits, bytes))]
pub fn bitwrap_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);

    if let syn::Data::Struct(ref s) = input.data {
        let mut bitwrap = BitWrapMacro::new(&input.ident);
        bitwrap.build(s).into()
    } else {
        panic!("struct required")
    }
}
