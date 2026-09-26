use boltffi_binding::{CodecNode, Decl, OwnedWireEncoding, RecordDecl};
use proc_macro2::TokenStream;
use quote::quote;

use crate::expansion::{contract::Expansion, error::Error};

pub struct Value<'expansion, 'lowered, S: boltffi_binding::SurfaceLower> {
    codec: &'lowered CodecNode,
    expansion: &'expansion Expansion<'lowered, S>,
}

impl<'expansion, 'lowered, S: boltffi_binding::SurfaceLower> Value<'expansion, 'lowered, S> {
    pub const fn new(
        codec: &'lowered CodecNode,
        expansion: &'expansion Expansion<'lowered, S>,
    ) -> Self {
        Self { codec, expansion }
    }

    pub fn buffer(&self, value: TokenStream) -> Result<TokenStream, Error> {
        super::require_runtime_wire(self.codec)?;
        if self.has_class_handle_record() {
            return Ok(quote! { #value.__boltffi_wire_encode_owned() });
        }
        let conversion = super::custom::Outgoing::new(self.codec, self.expansion);
        if !conversion.has_custom_conversion() {
            return Ok(Self::owned_buffer(self.codec.owned_wire_encoding(), value));
        }
        let value = conversion.convert(value)?;
        let buffer =
            Self::owned_buffer(self.codec.owned_wire_encoding(), quote! { __boltffi_wire });
        Ok(quote! {
            {
                let __boltffi_wire = #value;
                #buffer
            }
        })
    }

    pub fn borrowed_buffer(&self, value: TokenStream) -> Result<TokenStream, Error> {
        super::require_runtime_wire(self.codec)?;
        if self.has_class_handle_record() {
            return Err(Error::UnsupportedExpansion(
                "borrowed record containing class handles",
            ));
        }
        let conversion = super::custom::BorrowedOutgoing::new(self.codec, self.expansion);
        if !conversion.has_custom_conversion() {
            return Ok(quote! { ::boltffi::__private::FfiBuf::wire_encode(&#value) });
        }
        let value = conversion.convert(value)?;
        Ok(quote! {
            {
                let __boltffi_wire = #value;
                ::boltffi::__private::FfiBuf::wire_encode(&__boltffi_wire)
            }
        })
    }

    fn has_class_handle_record(&self) -> bool {
        let CodecNode::EncodedRecord(id) = self.codec else {
            return false;
        };
        self.expansion.bindings().decls().iter().any(|declaration| {
            matches!(declaration, Decl::Record(record)
                if matches!(record.as_ref(), RecordDecl::Encoded(record)
                    if record.id() == *id && record.fields().iter().any(|field|
                        matches!(field.codec().write().root(), CodecNode::ClassHandle(_)))))
        })
    }

    fn owned_buffer(encoding: OwnedWireEncoding, value: TokenStream) -> TokenStream {
        match encoding {
            OwnedWireEncoding::String => {
                quote! { ::boltffi::__private::FfiBuf::wire_encode_owned_string(#value) }
            }
            OwnedWireEncoding::Utf8String => {
                quote! { ::boltffi::__private::FfiBuf::from_vec(#value.into_bytes()) }
            }
            OwnedWireEncoding::Bytes => {
                quote! { ::boltffi::__private::FfiBuf::wire_encode_owned_bytes(#value) }
            }
            OwnedWireEncoding::RawBytes => {
                quote! { ::boltffi::__private::FfiBuf::from_vec(#value) }
            }
            _ => {
                quote! { ::boltffi::__private::FfiBuf::wire_encode(&#value) }
            }
        }
    }
}
