//! Generated code that reads and writes one field.

use proc_macro2::{Literal, Span, TokenStream};
use quote::quote;

use layline_codegen::{Endian, Root};

use super::{Body, Packed, Plan, Prim, Reading};

pub(super) enum Access {
    /// The whole layout loaded as one `u128`, indexed through `__LAYLINE_LENSES`.
    Container { idx: usize, wide: bool },
    /// The whole layout loaded as one native register of `reg` bytes, shifted and masked.
    Register { start: u32, width: u32, reg: usize },
    /// Only the bytes a field touches, for layouts too wide to load whole.
    Window {
        /// First wire byte.
        offset: usize,
        /// Byte count, `1..=16`.
        len: usize,
        /// Integer width in bytes: `len` rounded up to a power of two, or 16 for `u128`.
        reg: usize,
        /// The field's bit offset within its first byte.
        shift: u32,
        /// `width` ones.
        mask: u128,
        /// Whether the field is a `u128`.
        wide: bool,
        /// A 17th byte out of the integer's reach, as `(wire byte, field bits in it)`.
        tail: Option<(usize, u32)>,
    },
    /// A bit field inside one fixed-size word, shifted and masked.
    Region { ty: Prim, byte: usize, off: u32, mask: u64 },
    /// A whole scalar at a byte offset.
    Whole { ty: Prim, offset: usize },
}

pub(super) struct Writing<'a> {
    pub(super) place: &'a Place,
    /// The field's index into `__LAYLINE_LENSES`.
    pub(super) idx: usize,
    pub(super) name: &'a syn::Ident,
    pub(super) fits_guard: bool,
}

pub(super) struct Place {
    /// The byte array: `wire`, `__out` or `self.0`.
    pub(super) buf: TokenStream,
    /// The same bytes as a reference to the whole array.
    pub(super) buf_ref: TokenStream,
    /// Decode and encode load the layout into a local once. View accessors load it on each call.
    pub(super) hoisted: bool,
}

impl Access {
    /// Whether the access works in a register of exactly `bits` bits, so a cast would be a no-op.
    fn register_is(&self, bits: u32) -> bool {
        matches!(self, Access::Register { reg, .. } if *reg as u32 * 8 == bits)
    }

    fn u64_raw(&self) -> bool {
        matches!(self, Access::Container { wide: false, .. }) || self.register_is(64)
    }

    fn wide(&self) -> bool {
        matches!(self, Access::Container { wide: true, .. } | Access::Window { wide: true, .. })
    }

    fn raw_ty(&self) -> TokenStream {
        match self {
            Access::Container { wide: true, .. } => quote!(::core::primitive::u128),
            Access::Container { .. } => quote!(::core::primitive::u64),
            Access::Window { reg, .. } | Access::Register { reg, .. } => {
                let i = win_ty(*reg);
                quote!(::core::primitive::#i)
            }
            Access::Region { ty, .. } | Access::Whole { ty, .. } => {
                let i = ty.ident();
                quote!(::core::primitive::#i)
            }
        }
    }
}

impl Plan<'_> {
    pub(super) fn hoisted(&self) -> bool {
        self.grid.is_bit_addressed() && self.grid.interior_boundary().is_none() && self.bits <= 128
    }

    /// The native register a word-sized `bits = N` layout loads into, in bytes: `N / 8` rounded
    /// up to 1, 2, 4 or 8. `None` for a layout wider than 64 bits, which loads into a `u128`.
    pub(super) fn register(&self) -> Option<usize> {
        (self.hoisted() && self.bits <= 64).then(|| (self.wire_bytes).next_power_of_two())
    }

    /// The register type, when the register is exactly the wire size and so has no unused bits.
    pub(super) fn word_ty(&self) -> Option<TokenStream> {
        let reg = self.register().filter(|&reg| reg == self.wire_bytes)?;
        let ty = win_ty(reg);
        Some(quote!(::core::primitive::#ty))
    }

    /// The register loaded from the wire bytes in `buf`.
    pub(super) fn register_load(&self, buf: &TokenStream) -> TokenStream {
        let reg = self.register().expect("a register layout");
        self.window_bytes(0, self.wire_bytes, reg, buf)
    }

    /// The wire bytes of the register in `__bits`.
    pub(super) fn register_store(&self) -> TokenStream {
        let reg = self.register().expect("a register layout");
        let n = self.wire_bytes;
        if reg == 1 {
            return quote!([__bits]);
        }
        let (_, to) = byte_fns(self.endian);
        if reg == n {
            return quote!(__bits.#to());
        }
        let first = match self.endian {
            Endian::Le => 0,
            Endian::Be => reg - n,
        };
        let picks = (first..first + n).map(|i| quote!(__b[#i]));
        quote! {{
            let __b = __bits.#to();
            [ #(#picks),* ]
        }}
    }

    pub(super) fn container_window(&self, at: u64, width: u64) -> (usize, usize) {
        let first = (at / 8) as usize;
        let last = ((at + width - 1) / 8) as usize;
        (first, last - first + 1)
    }

    pub(super) fn wire_offset(&self, c0: usize, len: usize) -> usize {
        match self.endian {
            Endian::Le => c0,
            Endian::Be => self.wire_bytes - c0 - len,
        }
    }

    fn window(&self, at: u64, p: &Packed) -> Access {
        let (c0, span) = self.container_window(at, u64::from(p.bits));
        let wide = p.wide();
        let shift = (at % 8) as u32;
        let mask = wide_mask_of(p.bits);
        let reg = if wide { 16 } else { span.next_power_of_two() };
        if span <= 16 {
            return Access::Window {
                offset: self.wire_offset(c0, span),
                len: span,
                reg,
                shift,
                mask,
                wide,
                tail: None,
            };
        }
        ::core::assert!(span == 17 && wide, "only a 122+-bit carry can span 17 bytes");
        Access::Window {
            offset: self.wire_offset(c0, 16),
            len: 16,
            reg: 16,
            shift,
            mask,
            wide,
            tail: Some((self.wire_offset(c0 + 16, 1), p.bits - (128 - shift))),
        }
    }

    fn access(&self, at: u64, p: &Packed, idx: usize, place: &Place) -> Access {
        if let Reading::Native(ty) = p.reading {
            return Access::Whole { ty, offset: (at / 8) as usize };
        }
        match (self.grid.interior_boundary(), self.grid.is_bit_addressed()) {
            (Some(region), _) => {
                let ty = uint_scalar(region as u32);
                Access::Region {
                    ty,
                    byte: ((at - at % region) / 8) as usize,
                    off: (at % region) as u32,
                    mask: mask_of(p.bits),
                }
            }
            (None, true) if self.hoisted() && place.hoisted => match self.register() {
                Some(reg) => Access::Register { start: at as u32, width: p.bits, reg },
                None => Access::Container { idx, wide: p.wide() },
            },
            (None, true) => self.window(at, p),
            (None, false) => Access::Whole { ty: uint_scalar(p.bits), offset: (at / 8) as usize },
        }
    }

    pub(super) fn load_prologue(&self, place: &Place) -> TokenStream {
        let root = self.root;
        if self.register().is_some() {
            let load = self.register_load(&place.buf);
            quote! { let __raw = #load; }
        } else if self.hoisted() {
            let (from, _) = wire_fns(self.endian);
            let (bits, wire_bytes) = (self.bits, self.wire_bytes);
            let buf_ref = &place.buf_ref;
            quote! {
                let __raw = #root::__private::BitWord::<#bits, #wire_bytes>::#from(#buf_ref).raw();
            }
        } else {
            quote!()
        }
    }

    pub(super) fn window_bytes(
        &self,
        offset: usize,
        len: usize,
        reg: usize,
        buf: &TokenStream,
    ) -> TokenStream {
        if reg == 1 {
            return quote! { #buf[#offset] };
        }
        let ty = win_ty(reg);
        let ty = quote!(::core::primitive::#ty);
        let (from, _) = byte_fns(self.endian);
        let real = (0..len).map(|i| {
            let at = offset + i;
            quote! { #buf[#at] }
        });
        let pad = (0..reg - len).map(|_| quote!(0u8));
        match self.endian {
            Endian::Le => quote! { #ty::#from([ #(#real,)* #(#pad),* ]) },
            Endian::Be => quote! { #ty::#from([ #(#pad,)* #(#real),* ]) },
        }
    }

    fn load(&self, a: &Access, place: &Place) -> TokenStream {
        let root = self.root;
        match a {
            Access::Container { idx, wide } => {
                let lens = self.lens(*idx);
                let raw = if place.hoisted {
                    quote!(__raw)
                } else {
                    let (from, _) = wire_fns(self.endian);
                    let (bits, wire_bytes) = (self.bits, self.wire_bytes);
                    let buf_ref = &place.buf_ref;
                    quote! { #root::__private::BitWord::<#bits, #wire_bytes>::#from(#buf_ref).raw() }
                };
                if *wide {
                    quote! { #lens.extract_wide(#raw) }
                } else {
                    quote! { #lens.extract(#raw) }
                }
            }
            Access::Register { start, width, reg } => {
                let mut read = quote!(__raw);
                if *start != 0 {
                    read = quote! { (#read >> #start) };
                }
                if start + width < *reg as u32 * 8 {
                    let mask = win_literal(*reg, wide_mask_of(*width));
                    read = quote! { (#read & #mask) };
                }
                read
            }
            Access::Window { offset, len, reg, shift, mask, tail, .. } => {
                let mut read = self.window_bytes(*offset, *len, *reg, &place.buf);
                if *shift != 0 {
                    read = quote! { (#read >> #shift) };
                }
                if let Some((byte, _)) = tail {
                    let up = 128 - *shift;
                    let buf = &place.buf;
                    read = quote! { (#read | ((#buf[#byte] as ::core::primitive::u128) << #up)) };
                }
                if *mask != reg_ones(*reg) {
                    let mask = win_literal(*reg, *mask);
                    read = quote! { (#read & #mask) };
                }
                read
            }
            Access::Region { ty, byte, off, mask } => {
                let load = scalar_read(*ty, *byte, &place.buf, self.endian);
                let mask = uint_literal(*ty, *mask);
                quote! { ((#load >> #off) & #mask) }
            }
            Access::Whole { ty, offset } => scalar_read(*ty, *offset, &place.buf, self.endian),
        }
    }

    pub(super) fn store(&self, a: &Access, place: &Place, raw: TokenStream) -> TokenStream {
        let root = self.root;
        match a {
            Access::Container { idx, wide } => {
                let lens = self.lens(*idx);
                let insert = if *wide { quote!(insert_wide) } else { quote!(insert) };
                if place.hoisted {
                    return quote! { __bits = #lens.#insert(__bits, #raw); };
                }
                let (from, to) = wire_fns(self.endian);
                let (bits, wire_bytes) = (self.bits, self.wire_bytes);
                let (buf, buf_ref) = (&place.buf, &place.buf_ref);
                quote! {
                    let mut __w = #root::__private::BitWord::<#bits, #wire_bytes>::#from(#buf_ref);
                    __w.set_raw(#lens.#insert(__w.raw(), #raw));
                    #buf = __w.#to();
                }
            }
            Access::Register { start, width, reg } => {
                let value = if *width < *reg as u32 * 8 {
                    let mask = win_literal(*reg, wide_mask_of(*width));
                    quote! { ((#raw) & #mask) }
                } else {
                    quote! { (#raw) }
                };
                if *start == 0 {
                    quote! { __bits |= #value; }
                } else {
                    quote! { __bits |= #value << #start; }
                }
            }
            Access::Window { offset, len, reg, shift, mask, tail, .. } => {
                let (offset, len, reg, shift) = (*offset, *len, *reg, *shift);
                let buf = &place.buf;
                let (bind, raw) = match tail {
                    Some(_) => (quote! { let __v = #raw; }, quote!(__v)),
                    None => (quote!(), raw),
                };
                let mut value = if *mask == reg_ones(reg) {
                    quote! { (#raw) }
                } else {
                    let m = win_literal(reg, *mask);
                    quote! { ((#raw) & #m) }
                };
                if shift != 0 {
                    value = quote! { (#value << #shift) };
                }
                let keep = reg_ones(reg) & !(*mask << shift);
                let new = if keep == 0 {
                    value
                } else {
                    let keep = win_literal(reg, keep);
                    let cur = self.window_bytes(offset, len, reg, buf);
                    quote! { ((#cur & #keep) | #value) }
                };
                if reg == 1 {
                    return quote! { #buf[#offset] = #new; };
                }
                let (_, to) = byte_fns(self.endian);
                let bytes = if reg == len {
                    quote!(__new.#to())
                } else {
                    let first = match self.endian {
                        Endian::Le => 0,
                        Endian::Be => reg - len,
                    };
                    let picks = (first..first + len).map(|i| quote!(__b[#i]));
                    quote! {{
                        let __b = __new.#to();
                        [ #(#picks),* ]
                    }}
                };
                let put = put(buf, &quote!(#offset), len, &bytes);
                let odd = tail.map(|(byte, bits)| {
                    let down = 128 - shift;
                    let keep = Literal::u8_suffixed(!(((1u16 << bits) - 1) as u8));
                    let m = Literal::u8_suffixed(((1u16 << bits) - 1) as u8);
                    quote! {
                        #buf[#byte] = (#buf[#byte] & #keep)
                            | (((#raw >> #down) as ::core::primitive::u8) & #m);
                    }
                });
                quote! {{
                    #bind
                    let __new = #new;
                    #put
                    #odd
                }}
            }
            Access::Region { ty, byte, off, mask } => {
                let (_, to) = byte_fns(self.endian);
                let load = scalar_read(*ty, *byte, &place.buf, self.endian);
                let mask = uint_literal(*ty, *mask);
                let put = put(&place.buf, &quote!(#byte), ty.width(), &quote!(__new.#to()));
                quote! {{
                    let __cur = #load;
                    let __new = (__cur & !(#mask << #off)) | (((#raw) & #mask) << #off);
                    #put
                }}
            }
            Access::Whole { ty, offset } => {
                scalar_write(*ty, *offset, &place.buf, raw, self.endian)
            }
        }
    }

    fn lens(&self, idx: usize) -> TokenStream {
        let owner = &self.ident;
        quote! { #owner::__LAYLINE_LENSES[#idx] }
    }
}

pub(super) fn sign_extend(raw: TokenStream, width: u32, ty: &syn::Type) -> TokenStream {
    let shift = 64 - width;
    quote! {
        ((((#raw) as ::core::primitive::u64) << #shift) as ::core::primitive::i64 >> #shift) as #ty
    }
}

fn decode_value(p: &Packed, a: &Access, raw: TokenStream, root: &Root) -> TokenStream {
    let ty = &p.ty;
    match p.reading {
        Reading::Uint(_) if a.wide() => raw,
        Reading::Uint(prim) if a.register_is(prim) => raw,
        Reading::Native(_) => raw,
        Reading::Uint(_) => quote! { #raw as #ty },
        Reading::Int => sign_extend(raw, p.bits, ty),
        Reading::Bool | Reading::Codec => {
            let raw = widen(raw, a);
            quote! { <#ty as #root::FieldCodec>::from_raw(#raw) }
        }
    }
}

fn encode_raw(p: &Packed, a: &Access, value: &TokenStream, root: &Root) -> TokenStream {
    let ty = &p.ty;
    let raw_ty = a.raw_ty();
    match p.reading {
        Reading::Uint(_) if a.wide() => quote!(#value),
        Reading::Uint(prim) if a.register_is(prim) => quote!(#value),
        Reading::Native(_) => quote!(#value),
        Reading::Uint(_) => quote! { #value as #raw_ty },
        Reading::Int => quote! { (#value as ::core::primitive::i64) as #raw_ty },
        Reading::Bool | Reading::Codec => {
            let raw = quote! { <#ty as #root::FieldCodec>::to_raw(&#value) };
            if a.u64_raw() {
                raw
            } else {
                quote! { (#raw as #raw_ty) }
            }
        }
    }
}

fn widen(raw: TokenStream, a: &Access) -> TokenStream {
    if a.u64_raw() {
        raw
    } else {
        quote! { #raw as ::core::primitive::u64 }
    }
}

/// Whether encode must narrow the body's value to fit: a field narrower than its type.
pub(super) fn narrowed(body: &Body) -> bool {
    match body {
        Body::Packed(p) => match p.reading {
            Reading::Int | Reading::Codec => p.bits < 64,
            Reading::Uint(prim) => p.bits < prim,
            Reading::Bool | Reading::Native(_) => false,
        },
        Body::Nested { .. } => false,
        Body::Repeat { body, .. } => narrowed(body),
    }
}

/// Asserts the value fits its width. Otherwise the mask would silently encode a different value.
pub(super) fn fits_check(
    p: &Packed,
    value: &TokenStream,
    name: &syn::Ident,
    root: &Root,
) -> TokenStream {
    let width = p.bits;
    let msg = format!("field `{name}`: value does not fit #[bits({width})]");
    match p.reading {
        Reading::Int if width < 64 => {
            let lo = -(1i64 << (width - 1));
            let hi = (1i64 << (width - 1)) - 1;
            quote! {
                ::core::assert!(
                    (#value as ::core::primitive::i64) >= #lo
                        && (#value as ::core::primitive::i64) <= #hi,
                    #msg
                );
            }
        }
        Reading::Codec if width < 64 => {
            let ty = &p.ty;
            quote! {
                ::core::assert!(
                    <#ty as #root::FieldCodec>::to_raw(&#value) >> #width == 0,
                    #msg
                );
            }
        }
        Reading::Uint(prim) if width < prim => {
            let raw = if prim > 64 { quote!(u128) } else { quote!(u64) };
            quote! {
                ::core::assert!((#value as ::core::primitive::#raw) >> #width == 0, #msg);
            }
        }
        _ => quote!(),
    }
}

impl Plan<'_> {
    pub(super) fn read(&self, body: &Body, at: u64, idx: usize, place: &Place) -> TokenStream {
        match body {
            Body::Packed(p) => {
                let access = self.access(at, p, idx, place);
                decode_value(p, &access, self.load(&access, place), self.root)
            }
            Body::Nested { ty, bytes, .. } => {
                let (buf, a, b) = (&place.buf, (at / 8) as usize, (at / 8) as usize + bytes);
                if self.internal.0 {
                    let root = self.root;
                    quote! {
                        <#ty as #root::Layout>::decode_slice(&#buf[#a..#b])
                            .map_err(|__err| __err.rebased(#a))?
                    }
                } else {
                    quote! {
                        <#ty>::decode(
                            match <&[::core::primitive::u8; #bytes]>::try_from(&#buf[#a..#b]) {
                                ::core::result::Result::Ok(__sub) => __sub,
                                ::core::result::Result::Err(_) => &[0u8; #bytes],
                            },
                        )
                    }
                }
            }
            Body::Repeat { .. } if native_leaf(body) => {
                let off = Literal::usize_unsuffixed((at / 8) as usize);
                self.read_run(body, quote!(#off), 0, &place.buf)
            }
            Body::Repeat { body, len } => {
                let stride = body.width();
                let elems = (0..*len).map(|i| self.read(body, at + i as u64 * stride, idx, place));
                quote! { [ #(#elems),* ] }
            }
        }
    }

    pub(super) fn write(
        &self,
        body: &Body,
        at: u64,
        w: &Writing<'_>,
        value: &TokenStream,
    ) -> TokenStream {
        match body {
            Body::Packed(p) => {
                let access = self.access(at, p, w.idx, w.place);
                let raw = encode_raw(p, &access, value, self.root);
                let guard =
                    if w.fits_guard { fits_check(p, value, w.name, self.root) } else { quote!() };
                let store = self.store(&access, w.place, raw);
                quote! { #guard #store }
            }
            Body::Nested { bytes, .. } => {
                let (buf, a) = (&w.place.buf, (at / 8) as usize);
                put(buf, &quote!(#a), *bytes, &quote!(#value.encode()))
            }
            Body::Repeat { .. } if native_leaf(body) => {
                let off = Literal::usize_unsuffixed((at / 8) as usize);
                self.write_run(body, quote!(#off), 0, &w.place.buf, value)
            }
            Body::Repeat { body, len } => {
                let stride = body.width();
                let writes = (0..*len).map(|i| {
                    let elem = quote! { #value[#i] };
                    self.write(body, at + i as u64 * stride, w, &elem)
                });
                quote! { #(#writes)* }
            }
        }
    }
}

/// Whether a body is an array of scalars, which reads and writes as a run of bytes.
fn native_leaf(body: &Body) -> bool {
    match body {
        Body::Packed(Packed { reading: Reading::Native(_), .. }) => true,
        Body::Repeat { body, .. } => native_leaf(body),
        _ => false,
    }
}

impl Plan<'_> {
    /// Reads a scalar array at the byte offset `off`: one copy for bytes, a loop otherwise.
    fn read_run(
        &self,
        body: &Body,
        off: TokenStream,
        depth: usize,
        buf: &TokenStream,
    ) -> TokenStream {
        match body {
            Body::Packed(Packed { reading: Reading::Native(s), .. }) => {
                self.scalar_at(*s, &off, buf)
            }
            Body::Repeat { body: inner, len }
                if matches!(
                    **inner,
                    Body::Packed(Packed { reading: Reading::Native(Prim::U8), .. })
                ) =>
            {
                // A fixed-size subarray of a fixed-size array: the length always matches, so the
                // fallback is dead code and no panic path or temporary remains.
                quote! {
                    match <&[::core::primitive::u8; #len]>::try_from(&#buf[#off..#off + #len]) {
                        ::core::result::Result::Ok(__run) => *__run,
                        ::core::result::Result::Err(_) => [0u8; #len],
                    }
                }
            }
            Body::Repeat { body: inner, .. } => {
                let i = quote::format_ident!("__i{}", depth);
                let stride = (inner.width() / 8) as usize;
                let elem = self.read_run(inner, quote!(__at), depth + 1, buf);
                quote! {
                    ::core::array::from_fn(|#i| {
                        let __at = #off + #i * #stride;
                        #elem
                    })
                }
            }
            _ => unreachable!("native_leaf admits only scalar arrays"),
        }
    }

    fn scalar_at(&self, s: Prim, off: &TokenStream, buf: &TokenStream) -> TokenStream {
        match s {
            Prim::U8 => quote! { #buf[#off] },
            Prim::I8 => quote! { #buf[#off] as ::core::primitive::i8 },
            _ => {
                let ty = s.ident();
                let (from, _) = byte_fns(self.endian);
                let bytes = (0..s.width()).map(|i| quote! { #buf[#off + #i] });
                quote! { ::core::primitive::#ty::#from([ #(#bytes),* ]) }
            }
        }
    }

    /// Writes a scalar array at the byte offset `off`, as [`read_run`](Self::read_run) reads it.
    fn write_run(
        &self,
        body: &Body,
        off: TokenStream,
        depth: usize,
        buf: &TokenStream,
        value: &TokenStream,
    ) -> TokenStream {
        match body {
            Body::Packed(Packed { reading: Reading::Native(s), .. }) => match s {
                Prim::U8 => quote! { #buf[#off] = #value; },
                Prim::I8 => quote! { #buf[#off] = #value as ::core::primitive::u8; },
                _ => {
                    let (_, to) = byte_fns(self.endian);
                    let w = s.width();
                    put(buf, &off, w, &quote!(#value.#to()))
                }
            },
            Body::Repeat { body: inner, len }
                if matches!(
                    **inner,
                    Body::Packed(Packed { reading: Reading::Native(Prim::U8), .. })
                ) =>
            {
                quote! {
                    if let ::core::result::Result::Ok(__run) =
                        <&mut [::core::primitive::u8; #len]>::try_from(&mut #buf[#off..#off + #len])
                    {
                        *__run = #value;
                    }
                }
            }
            Body::Repeat { body: inner, len } => {
                let i = quote::format_ident!("__i{}", depth);
                let stride = (inner.width() / 8) as usize;
                let elem = self.write_run(inner, quote!(__at), depth + 1, buf, &quote!(#value[#i]));
                quote! {
                    for #i in 0..#len {
                        let __at = #off + #i * #stride;
                        #elem
                    }
                }
            }
            _ => unreachable!("native_leaf admits only scalar arrays"),
        }
    }
}

/// Stores the `[u8; n]` array `bytes` at byte `at` of `buf`.
///
/// A fixed-size subarray of a fixed-size array always matches, so unlike `copy_from_slice`, which
/// a size-optimized build leaves out of line, no length check or panic path remains.
pub(super) fn put(
    buf: &TokenStream,
    at: &TokenStream,
    n: usize,
    bytes: &TokenStream,
) -> TokenStream {
    quote! {
        if let ::core::result::Result::Ok(__dst) =
            <&mut [::core::primitive::u8; #n]>::try_from(&mut #buf[#at..#at + #n])
        {
            *__dst = #bytes;
        }
    }
}

pub(super) fn wire_fns(endian: Endian) -> (TokenStream, TokenStream) {
    match endian {
        Endian::Le => (quote!(from_wire), quote!(to_wire)),
        Endian::Be => (quote!(from_wire_be), quote!(to_wire_be)),
    }
}

pub(super) fn byte_fns(endian: Endian) -> (TokenStream, TokenStream) {
    match endian {
        Endian::Le => (quote!(from_le_bytes), quote!(to_le_bytes)),
        Endian::Be => (quote!(from_be_bytes), quote!(to_be_bytes)),
    }
}

fn mask_of(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 }
}

pub(super) fn wide_mask_of(bits: u32) -> u128 {
    if bits >= 128 { u128::MAX } else { (1u128 << bits) - 1 }
}

fn reg_ones(reg: usize) -> u128 {
    wide_mask_of(reg as u32 * 8)
}

pub(super) fn win_name(reg: usize) -> &'static str {
    match reg {
        1 => "u8",
        2 => "u16",
        4 => "u32",
        8 => "u64",
        _ => "u128",
    }
}

pub(super) fn win_ty(reg: usize) -> syn::Ident {
    syn::Ident::new(win_name(reg), Span::call_site())
}

pub(super) fn win_literal(reg: usize, v: u128) -> Literal {
    match reg {
        1 => Literal::u8_suffixed(v as u8),
        2 => Literal::u16_suffixed(v as u16),
        4 => Literal::u32_suffixed(v as u32),
        8 => Literal::u64_suffixed(v as u64),
        _ => Literal::u128_suffixed(v),
    }
}

fn uint_scalar(bits: u32) -> Prim {
    match bits {
        8 => Prim::U8,
        16 => Prim::U16,
        32 => Prim::U32,
        _ => Prim::U64,
    }
}

fn uint_literal(s: Prim, v: u64) -> Literal {
    match s {
        Prim::U8 => Literal::u8_suffixed(v as u8),
        Prim::U16 => Literal::u16_suffixed(v as u16),
        Prim::U32 => Literal::u32_suffixed(v as u32),
        _ => Literal::u64_suffixed(v),
    }
}

fn scalar_read(s: Prim, offset: usize, buf: &TokenStream, endian: Endian) -> TokenStream {
    match s {
        Prim::U8 => quote! { #buf[#offset] },
        Prim::I8 => quote! { #buf[#offset] as ::core::primitive::i8 },
        _ => {
            let ty = s.ident();
            let ty = quote!(::core::primitive::#ty);
            let (from, _) = byte_fns(endian);
            let bytes = (0..s.width()).map(|i| {
                let at = offset + i;
                quote! { #buf[#at] }
            });
            quote! { #ty::#from([ #(#bytes),* ]) }
        }
    }
}

fn scalar_write(
    s: Prim,
    offset: usize,
    buf: &TokenStream,
    value: TokenStream,
    endian: Endian,
) -> TokenStream {
    let (_, to) = byte_fns(endian);
    match s {
        Prim::U8 => quote! { #buf[#offset] = #value; },
        Prim::I8 => quote! { #buf[#offset] = #value as ::core::primitive::u8; },
        _ => put(buf, &quote!(#offset), s.width(), &quote!(#value.#to())),
    }
}
