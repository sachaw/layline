//! Derive macros for `layline`: `Layout`, `Message`, `FieldCodec` and `Dispatch`.
//!
//! Each attribute, and each key inside it, may appear once.
//! A rejected type still implements its trait, so the build reports only the real error.

use proc_macro::TokenStream;

mod attr;
mod check;
mod claim;
mod codec;
mod dispatch;
mod layout;
mod message;
#[cfg(test)]
mod refusals;
mod root;
mod stub;
mod ty;
mod width;

fn refused(err: syn::Error, stub: proc_macro2::TokenStream) -> TokenStream {
    let mut out = err.to_compile_error();
    out.extend(stub);
    out.into()
}

/// Derives `Layout` for a fixed-size struct.
///
/// Fields are laid out in declaration order and must cover every bit.
/// `decode` returns `Result<Self, ParseError>` if the struct uses `#[magic]`, `#[checksum]`,
/// `#[range]`, `prefix_value` or `internal`. Otherwise it returns `Self`.
///
/// # Container attributes
///
/// Choose one size:
///
/// - `#[layout(bits = N)]`: `N` bits of bit fields. `N` is a multiple of 8.
/// - `#[layout(bytes = N)]`: `N` bytes of scalars, arrays and nested layouts.
/// - `#[layout(words = N)]`: `N` 16-bit words of bit fields.
///
/// Optional keys:
///
/// - `endian = le | be`: byte order. Default `le`.
/// - `order = lsb | msb`: bit order for `bits` and `words`. With `msb`, the first field takes the
///   most significant bits. Default `lsb`.
/// - `prefix = K`: the low `K` bits hold a `Dispatch` id. Sets `Layout::PREFIX_BITS`. `bits` only.
/// - `prefix_value = V`: the prefix value. Encode writes it and decode checks it. Needs `prefix`.
/// - `view`: also generates `<Name>View`, which reads and writes the fields in place in a byte
///   buffer. Not allowed with `#[magic]`, `#[checksum]` or `#[range]`.
/// - `internal`: marks a block that `Message` generates. Makes `decode` fallible.
/// - `crate = path`: the path to `layline`. Default `::layline`.
///
/// # Field attributes
///
/// - `#[bits(N)]`: the width in bits, for `bits` and `words`. The type is an integer, `bool`,
///   `U<N>`, `I<N>`, or a `FieldCodec` whose `BITS` is `N`. In `words`, a field fits in one word.
/// - `#[codec(N)]`: a `FieldCodec` stored as an 8, 16, 32 or 64-bit integer, or an array of
///   them. `bytes` only.
/// - `#[bytes(N)]`: a nested layout of `N` bytes, or an array of them. `N` must match its
///   `WIRE_BYTES`. `bytes` and `words` only.
/// - `#[at(bit = N)]`, `#[at(byte = N)]`: the field's expected position. The build fails if the
///   widths put it elsewhere. `STATED` lists these positions.
/// - `#[overlay(name: T)]`: adds `name` and `set_name`, which read and write the field's bits as
///   the `FieldCodec` `T`. `bits` and `words` only.
/// - `#[magic(0xAA55)]`, `#[magic(b"\x7fELF")]`: fixed bytes. Decode checks them and encode
///   writes them. `bytes` only.
/// - `#[checksum(Alg, over = a..=b)]`: a checksum over fields `a` to `b`. Decode verifies it and
///   encode computes it. `bytes` only.
/// - `#[range(lo..=hi)]`, `#[range(..=hi)]`, `#[range(lo..)]`: the allowed values. Decode rejects
///   others.
///
/// `Layout` rejects the `Message` attributes `#[count]`, `#[len]`, `#[stride]`, `#[fill]`,
/// `#[seek]`, `#[text]`, `#[until]`, `#[var]`, `#[message]`, `#[switch]` and `#[when]` with a
/// clear error.
///
/// ```
/// # use layline::{FieldCodec, Layout};
/// # #[derive(FieldCodec, Debug, Clone, PartialEq)] #[bits(4)] pub struct State(pub u8);
/// #[derive(Layout)]
/// #[layout(bits = 32)]
/// pub struct StatusWord {
///     #[bits(3)]  pub version: u8,
///     #[bits(1)]  pub urgent: bool,
///     #[bits(12)] #[at(bit = 4)] pub sequence: u16,
///     #[bits(4)]  pub state: State,
///     #[bits(10)] pub payload_len: u16,
///     #[bits(2)]  pub spare: u8,
/// }
/// ```
#[proc_macro_derive(
    Layout,
    attributes(
        layout, bits, codec, bytes, at, overlay, magic, checksum, range, count, len, stride, fill,
        seek, text, until, var, message, switch, when
    )
)]
pub fn derive_layout(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match layout::derive(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => refused(err, stub::layout(&input)),
    }
}

/// Derives `Dispatch` for an enum of `Layout` payloads selected by an id.
///
/// A payload type used by only one variant also gets `From<T>` and `Slot<T>`.
///
/// # Container attributes
///
/// - `#[dispatch(id = u16)]`: the id type.
/// - `prefix = K`: the id is the low `K` bits of each payload. Sets `Dispatch::PREFIX_BITS`.
///   Each payload must declare `#[layout(prefix = K)]`.
/// - `crate = path`: the path to `layline`. Default `::layline`.
///
/// # Variant attributes
///
/// - `#[value(N)]`: the variant's id.
/// - `#[other]`: the fallback `Unknown { id, body }`, for unknown ids and payloads that fail to
///   decode. `body` is `&'a [u8]` if the enum has a lifetime, and `Vec<u8>` otherwise.
///
/// ```
/// # use layline::{Dispatch, Layout};
/// # #[derive(Layout, Debug, Clone, PartialEq)] #[layout(bytes = 2)] pub struct Sample { pub a: u16 }
/// # #[derive(Layout, Debug, Clone, PartialEq)] #[layout(bytes = 2)] pub struct TimePulse { pub a: u16 }
/// #[derive(Dispatch)]
/// #[dispatch(id = u16)]
/// pub enum Payload<'a> {
///     #[value(4)]  Sample(Sample),
///     #[value(13)] TimePulse(TimePulse),
///     #[other]     Unknown { id: u16, body: &'a [u8] },
/// }
/// ```
#[proc_macro_derive(Dispatch, attributes(dispatch, value, other))]
pub fn derive_dispatch(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match dispatch::derive(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => refused(err, stub::dispatch(&input)),
    }
}

/// Derives `FieldCodec` and `WireInt` for an integer newtype or an enum of named values.
///
/// Every `N`-bit value decodes.
///
/// # Container attributes
///
/// - `#[bits(N)]`: the width, 1 to 64. The newtype's integer and every variant's value must fit.
/// - `#[bits(N, crate = path)]`: the path to `layline`. Default `::layline`.
///
/// # Variant attributes
///
/// - `#[value(N)]`: the raw value of a unit variant.
/// - `#[other]`: a one-field variant, over an unsigned integer, for every unlisted value. Without
///   it, the variants must list every `N`-bit value.
///
/// ```
/// # use layline::FieldCodec;
/// #[derive(FieldCodec)]
/// #[bits(15)]
/// pub struct TrackNumber(u16);
///
/// #[derive(FieldCodec)]
/// #[bits(3)]
/// pub enum Identity {
///     #[value(0)] Pending,
///     #[value(2)] Friend,
///     #[other] Undefined(u8),
/// }
/// ```
#[proc_macro_derive(FieldCodec, attributes(bits, value, other))]
pub fn derive_field_codec(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match codec::derive(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => refused(err, stub::field_codec(&input)),
    }
}

/// Derives `Message` for a variable-size struct or enum, or `Choice` for an enum that
/// `#[switch]` selects.
///
/// Consecutive plain fields become a hidden `#[derive(Layout)]` struct. Encode fills in count,
/// length, offset, discriminant and flag fields from the fields they describe.
///
/// # Container attributes
///
/// - `#[message(endian = le | be)]`: byte order. Default `le`.
/// - `#[message(bits)]`, `#[message(bits, order = lsb | msb)]`: all fields are bit fields, padded
///   with zero bits to a whole byte. Default order `lsb`.
/// - `#[message(needs(a: u8, b: u16))]`: parameters the parent message supplies. They become
///   `Message::Ctx`, a generated `<Name>Ctx` struct.
/// - `#[message(closed)]`: on an enum, an unknown discriminant fails with
///   `ParseError::Malformed`.
/// - `#[message(crate = path)]`: the path to `layline`. Default `::layline`.
///
/// # Collection attributes
///
/// A `Vec<T>` or `Box<[T]>` field takes one of these:
///
/// - `#[count(f)]`: `f` elements. `f` is an earlier field, or `hdr.f` in an earlier nested layout.
/// - `#[count(f * f)]`: `f` squared elements.
/// - `#[count(f, scale = S, offset = O, cap = N)]`: `f * S + O` elements. Decode rejects more
///   than `N`.
/// - `#[len(f)]`: `f` bytes. Takes the same keys. Also sizes a `#[text]` string or a `#[switch]`
///   body.
/// - `#[fill]`, `#[fill(cap = N)]`: whole elements until the message ends.
/// - `#[until(b)]`: bytes up to the terminator `b`. Decode consumes it and encode appends it.
/// - `#[until(mask = M)]`: elements up to and including the first whose first byte has the `M`
///   bits set.
///
/// # Field attributes
///
/// - `#[stride(f)]`, `#[stride(f, scale = S, offset = O)]`: the byte size of one element. Use with
///   `#[count]`.
/// - `#[seek(f)]`: read at the byte offset in `f`, counted from the start of the message. An
///   `Option<T>` at offset zero is `None`.
/// - `#[at(byte = N)]`: the byte offset where the field starts.
/// - `#[bytes(N)]`: the size of a nested layout, one element, a `#[switch]` body, or a NUL-padded
///   `#[text]` string.
/// - `#[codec(N)]`: a `FieldCodec` stored as an 8, 16, 32 or 64-bit integer.
/// - `#[var]`: a `VarCodec` value, whose size is read from the wire.
/// - `#[text]`, `#[text(Codec)]`: a `String`, as UTF-8 or through the `TextCodec` `Codec`.
/// - `#[message]`: a nested `Message`, boxed or not.
/// - `#[with(f, g)]`: earlier fields passed, in order, as the nested message's `needs`. Use with
///   `#[message]`.
/// - `#[switch(f)]`: an enum whose variant `f` selects. `#[switch(..)]` selects by the remaining
///   length.
/// - `#[when(flags & MASK)]`, `#[when(flag)]`: an `Option<T>` present when the bits are set.
///   Encode sets them for `Some` and clears them for `None`.
/// - `#[when(n > 0)]`: an `Option<T>` present when `n` is non-zero. Use with `#[seek]`.
/// - `#[fill]` on an `Option<T>`: present when bytes remain.
/// - `#[magic(0xFE)]`, `#[magic(b"SEAL")]`: fixed bytes. Decode checks them and encode writes
///   them.
/// - `#[checksum(Alg, over = a..=b)]`: a checksum over fields `a` to `b`. Decode verifies it and
///   encode computes it.
/// - `#[range(lo..=hi)]`: the allowed values. Decode rejects others.
///
/// # Bit message attributes
///
/// With `#[message(bits)]`:
///
/// - `#[bits(N)]`: the width in bits, 1 to 64. `bool` is 1 bit. `U<N>` and `I<N>` are `N` bits.
/// - `#[present]`: an `Option<T>` preceded by its own presence bit.
/// - `#[when(..)]`: as above.
/// - `#[var]`: a `BitCodec` value, whose width is read from the wire.
///
/// # Variant attributes
///
/// - `#[value(N)]`: the discriminant. The payload is a `Message`, a scalar, or a `#[bytes(N)]`
///   layout.
/// - `#[other]`: a one-field variant for every unlisted discriminant. It holds `Vec<u8>`, or
///   `[u8; N]` in a fixed-size enum.
///
/// ```
/// # use layline::{Layout, Message};
/// # #[derive(Message, Debug, Clone, PartialEq)] #[message(closed)]
/// # pub enum Body { #[value(0)] A(u8), #[value(1)] B(u8) }
/// # #[derive(Layout, Debug, Clone, PartialEq)] #[layout(bytes = 2)] pub struct Entry { pub a: u16 }
/// #[derive(Layout, Debug, Clone, PartialEq)]
/// #[layout(bits = 24)]
/// pub struct Header {
///     #[bits(2)]  pub kind: u8,
///     #[bits(4)]  pub n_entries: u8,
///     #[bits(2)]  pub flags: u8,
///     #[bits(6)]  pub table_offset: u8,
///     #[bits(10)] pub spare: u16,
/// }
///
/// #[derive(Message)]
/// pub struct Packet {
///     #[bytes(3)] pub header: Header,
///     #[when(header.flags & 0x01)] pub stamp: Option<u16>,
///     #[switch(header.kind)] pub body: Body,
///     #[count(header.n_entries)] #[seek(header.table_offset)] #[bytes(2)]
///     pub entries: Vec<Entry>,
/// }
/// ```
#[proc_macro_derive(
    Message,
    attributes(
        message, count, len, stride, fill, seek, at, codec, bits, bytes, text, until, var, switch,
        when, with, present, checksum, magic, value, other, range
    )
)]
pub fn derive_message(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match message::derive(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => refused(err, stub::message(&input)),
    }
}
