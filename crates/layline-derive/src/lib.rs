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
mod value;
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
/// `#[range]`, `prefix_value`, `internal` or a byte-mode `bool`, or nests a layout whose `decode`
/// does. Otherwise it returns `Self`.
///
/// `encode` is the same choice for values: it returns `Result<[u8; N], EncodeError>` if the struct
/// declares something a value can break, a `#[range]` or, without `truncate`, an integer narrower
/// than its type, and `[u8; N]` if it declares nothing and holds nothing that does. A struct that
/// nests a layout or holds a `FieldCodec` returns what they do, through `Encoded`.
/// `Layout::ENCODE_FALLIBLE` says which, as `DECODE_FALLIBLE` does for `decode`.
///
/// # Container attributes
///
/// Choose one size:
///
/// - `#[layout(bits = N)]`: `N` bits of bit fields. `N` is a multiple of 8.
/// - `#[layout(bytes = N)]`: `N` bytes of scalars, arrays and nested layouts. A `bool` is one
///   byte: decode reads 0 as `false` and 1 as `true` and refuses any other byte with
///   `ParseError::Malformed`, and encode writes 0 or 1. For a flag byte the format sets with any
///   nonzero value, declare `#[codec(8)] flag: layline::Nonzero`.
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
///   buffer. A view is unchecked, so a reader can inspect a corrupt record: `verify()` runs the
///   checks `decode` runs on magic bytes, `prefix_value` and checksums, `checked(&wire)` views
///   only bytes that pass them, and `seal()` rewrites the prefix, magic and checksums after the
///   setters. Not allowed with `#[range]`, a byte-mode `bool`, or a nested layout whose `decode`
///   is fallible.
/// - `truncate`: encode keeps the low `N` bits of a value wider than its `#[bits(N)]` field, as a
///   hardware register does, instead of refusing it. Decode is unchanged. Without it, encode
///   returns `EncodeError::Unfit` rather than write bits that read back as a different value. A
///   view's setters always keep the low bits, and a view of a struct whose `encode` can refuse
///   comes from it by `TryFrom`.
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
///   `WIRE_BYTES`. Decode runs its checks and returns its error. `bytes` and `words` only.
/// - `#[at(bit = N)]`, `#[at(byte = N)]`: the field's expected position. The build fails if the
///   widths put it elsewhere. `POSITIONS` lists these positions.
/// - `#[overlay(name: T)]`: adds `name` and `set_name`, which read and write the field's bits as
///   the `FieldCodec` `T`. `bits` and `words` only.
/// - `#[magic(0xAA55)]`, `#[magic(b"\x7fELF")]`: fixed bytes. Decode checks them and encode
///   writes them. `bytes` only.
/// - `#[checksum(Alg, over = a..=b)]`: a checksum over fields `a` to `b`. Decode verifies it and
///   encode computes it. `bytes` only.
/// - `#[range(lo..=hi)]`, `#[range(..=hi)]`, `#[range(lo..)]`: the allowed values. Decode rejects
///   others.
///
/// # Word-sized layouts
///
/// A `bits = N` layout with `N <= 64` loads into one native register (`u8`, `u16`, `u32` or `u64`,
/// the smallest that holds `N` bits), and `decode`, `encode` and the view's accessors are inline
/// shifts and masks on it. When `N` is 8, 16, 32 or 64 the layout also has
/// `from_bits(uN) -> Self` and `to_bits(&self) -> uN`, which skip the byte array: bit `n` of the
/// integer is container bit `n`, so `from_bits(u32::from_le_bytes(wire))` is `decode(&wire)` for
/// an `endian = le` layout. `from_bits` returns what `decode` returns.
///
/// `Layout` rejects the `Message` attributes `#[count]`, `#[len]`, `#[stride]`, `#[fill]`,
/// `#[seek]`, `#[text]`, `#[until]`, `#[var]`, `#[message]`, `#[switch]`, `#[when]` and
/// `#[align]` with a clear error.
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
        seek, text, until, var, message, switch, when, align, valid
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

/// Derives `FieldCodec` and `WireInt` for an integer newtype or an enum of named values, and
/// `FieldCodec` for a group of bit fields.
///
/// Every `N`-bit value decodes. A value whose raw bits do not fit `N`, a newtype holding more than
/// `N` bits or an `#[other]` value past them, has no encoding: `try_raw` refuses it with
/// `EncodeError::Unfit`, and `FieldCodec::ENCODE_FALLIBLE` is `true` for a type that can hold one.
///
/// # Container attributes
///
/// - `#[bits(N)]`: the width, 1 to 64. The newtype's integer and every variant's value must fit.
/// - `#[bits(N, crate = path)]`: the path to `layline`. Default `::layline`.
///
/// # Groups
///
/// A struct with named fields is a group: each field states `#[bits(K)]`, as in a
/// `#[layout(bits = N)]`, and the widths must add up to `N`. A group is a field narrower than a
/// whole number of bytes with fields of its own, so it can be an `#[overlay]` or a `#[bits(N)]`
/// field. Fields are an integer, `bool`, `U<K>`, `I<K>` or a `FieldCodec`. Group keys:
///
/// - `order = lsb | msb`: with `msb`, the first field takes the most significant bits. Default
///   `lsb`.
/// - `truncate`: `try_raw` and `to_bits` keep the low `K` bits of a value wider than its field,
///   instead of refusing it with `EncodeError::Unfit`. `to_raw` always keeps them.
///
/// ```
/// # use layline::FieldCodec;
/// #[derive(FieldCodec)]
/// #[bits(21)]
/// pub struct Descriptor {
///     #[bits(13)] pub words: u16,
///     #[bits(8)] pub buffer_index: u8,
/// }
///
/// assert_eq!(Descriptor { words: 1024, buffer_index: 42 }.to_raw(), 42 << 13 | 1024);
/// ```
///
/// # Variant attributes
///
/// - `#[value(N)]`: the raw value of a unit variant. An integer discriminant, `Variant = N`,
///   states it too. A variant with neither is refused: its position is not a value, unless the
///   enum is `#[repr(u8)]`, `#[repr(u16)]`, `#[repr(u32)]` or `#[repr(u64)]`. Rust then defines
///   it, as zero for the first variant and one past the variant before otherwise, so the raw value
///   is what `as` gives. An implicit variant after a `#[value(N)]` or `#[other]` one is refused,
///   since that variant's raw value is not its Rust discriminant.
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
/// # Framing
///
/// A length, count or presence flag describes another field and carries no information of its
/// own. Where it sits immediately before what it describes, it is no field: the attribute says
/// where it is, as `#[len(prefix = u16)]`, `#[count(prefix = u8)]` or `#[present]`. Where it sits
/// anywhere else, it keeps its place in declaration order, which is wire order, and its type says
/// what it is: `Stated<T>`. Encode writes it, decode reads it, and `PartialEq`, `Eq` and `Hash`
/// ignore it, so a decoded value equals the one built with `Stated::default()`. An attribute must
/// write it, as the next section says, or the type is refused. With `#[when(c)]`, a `Stated<T>`
/// is on the wire only when `c` holds, and holds zero otherwise. A plain integer field keeps its
/// own meaning: encode writes it where an attribute does, and otherwise takes the value's.
///
/// ```
/// use layline::{Message, Stated};
///
/// #[derive(Message, Debug, PartialEq, Default)]
/// #[message(endian = be)]
/// pub struct Header {
///     #[len(prefix = u8)]
///     #[text]
///     pub key: String,
///     #[len(prefix = u16, cap = 16384)]
///     pub value: Vec<u8>,
/// }
///
/// #[derive(Message, Debug, PartialEq, Default)]
/// #[message(endian = be)]
/// pub struct Batch {
///     pub size: Stated<u32>,
///     pub n: Stated<u8>,
///     #[count(n)]
///     #[message]
///     pub headers: Vec<Header>,
///     #[present]
///     #[len(prefix = u16)]
///     #[text]
///     pub note: Option<String>,
///     #[len(size - 5)]
///     pub rest: Vec<u8>,
/// }
///
/// let batch = Batch {
///     headers: vec![Header { key: "k".into(), value: vec![7] }],
///     rest: vec![1],
///     ..Batch::default()
/// };
/// let wire = batch.encode().unwrap();
/// assert_eq!(wire, [0, 0, 0, 6, 1, 1, b'k', 0, 1, 7, 0, 1]);
/// let (back, _) = Batch::decode(&wire).unwrap();
/// assert_eq!((back.size.get(), back.n.get()), (6, 1));
/// assert_eq!(back, batch);
/// ```
///
/// `encode` and `encode_into` return `Result<_, EncodeError>` when the message declares something
/// a value can break, or holds a type that does, and otherwise the bytes and
/// `Result<(), Overflow>`. What a value can break: a field narrower than its type, a `#[range]`, a
/// cap, a count, length, offset or discriminant field too narrow for what it states, an
/// expression it cannot state exactly or disagrees with, a terminator or mask inside the value,
/// and a parameter it disagrees with.
/// The error names the field, the declaration and the value. A held message counts as it
/// declares, wherever it leads: a type may hold itself, through `Box` or `Vec` or through other
/// types, and each one's constants are searched across that cycle when the crate is built, so a
/// cycle that refuses nothing is infallible. `Message::ENCODE_FALLIBLE` says which; the trait's
/// own methods always return `EncodeError`. In a byte-mode
/// message a `bool` is one strict byte, as in a `bytes = N` layout. A struct with no fields,
/// `struct Ready {}` or `struct Ready;`, is a message of no bytes, such as a frame whose selector
/// alone says what happened.
///
/// # Expressions
///
/// Every attribute that takes a value, `#[count]`, `#[len]`, `#[stride]`, `#[seek]`,
/// `#[switch]`, `#[when]` and `#[valid]`, takes one expression in one language. Its operands are
/// integer literals (`12`, `0x0f`, `0b1010`, `1_000`), constants (a `SCREAMING_CASE` name or a
/// path with `::`), earlier fields (`n`, or `hdr.n` in an earlier nested layout), parameters, and
/// parenthesised expressions. Its operators bind as Rust's do, loosest first:
///
/// | Operators | Operands |
/// |---|---|
/// | `\|\|`, then `&&` | conditions |
/// | `==` `!=` `<` `<=` `>` `>=`, not chained | integers, or two `bool`s for `==` and `!=` |
/// | `\|`, then `^`, then `&` | integers |
/// | `<<` `>>`, then `+` `-`, then `*` `/` `%` | integers |
/// | unary `!`, `-` | a `bool`; an integer |
///
/// A condition is a `bool`, or the result of `&`, `|` or `^`, which holds when any bit is set.
/// `!` takes a `bool`: on an integer, Rust would flip its bits rather than test them. A number
/// must read at least one field or parameter; a fixed size is the type's, as `[T; N]` or
/// `#[bytes(N)]`. Evaluation is in `i64` and total: an operation that overflows, divides by zero
/// or shifts by a count outside `0..64` has no value, and decode refuses the bytes with
/// `ParseError::Malformed`, naming the first field the expression reads, rather than wrapping.
/// `<<` multiplies by a power of two, so a set bit shifted out overflows. `/` and `%` round
/// toward zero, and `&&` and `||` short-circuit.
///
/// What encode does with an expression is decided when the type is built, by one rule: **what
/// encode can work backwards.** A field holds a value encode has: the caller's in a plain field,
/// or, in a `Stated<T>`, none until encode writes one.
///
/// - An expression that reads exactly one field, once, through steps that each have an inverse,
///   is written: encode works the number back to that field. The steps are `+ k`, `- k`,
///   `k - f`, `* k` (exactly), `/ k`, `<< k` (exactly), `>> k`, unary `-`, and `f * f` (an exact
///   root), where `k` reads only literals, constants and parameters. So `#[count(n * 2 - 1)]`
///   writes `n = (len + 1) / 2`. Where no value of the field gives the number, or decode would
///   read back a different one, encode refuses with `EncodeError::Inexact`; a value the field is
///   too narrow for is `EncodeError::Unfit`, and one past a `cap` is `EncodeError::OverCap`.
/// - A condition that tests one field is written too: `f` for a `bool`, `f & MASK` for bits,
///   with a literal or constant mask, and `!` or `== 0` for their absence. Encode sets or clears
///   them from whether the field is `Some`, keeping the field's other bits.
/// - Any other expression is checked. It reads each field as encode writes it: a plain field
///   holds the caller's value unless an attribute writes it, a `Stated<T>` only what an attribute
///   writes, and a parameter the holder's. Encode returns `EncodeError::Mismatch` where the value
///   disagrees with the expression, naming the field the attribute is on. An expression that
///   reads a field encode fills in only after a later segment, such as a `#[seek]` record's
///   offset, is refused when the type is built.
///
/// Where two attributes would write one field, the first writes it and the second is checked. A
/// `Stated<T>` that no attribute writes is refused when the type is built: `#[count(n * m)]` reads
/// two fields, so it writes neither. A `#[with(n)]` passes a written field as encode writes it.
/// An `#[other]` arm has no discriminant, so encode refuses one with `EncodeError::Unstated` when
/// a `Stated<T>` selects it. In a `Stated<T>` flags word, bits no `#[when]` tests are zero:
/// encode writes them so, and decode refuses them set.
///
/// # Container attributes
///
/// - `#[message(endian = le | be)]`: byte order. Default `le`.
/// - `#[message(endian = needs(p))]`: the byte order is the `bool` parameter `p`, big-endian when
///   `true`. Decode and encode pick it once per message, or per arm on an enum. The type's own
///   fixed fields follow it, counts and checksums included. A nested layout keeps its own
///   `endian`, and an integer `#[magic]` wider than a byte is refused: write its bytes.
/// - `#[message(bits)]`, `#[message(bits, order = lsb | msb)]`: all fields are bit fields, padded
///   with zero bits to a whole byte. Default order `lsb`.
/// - `#[message(needs(a: u8, b: bool))]`: integer and `bool` parameters the holder supplies.
///   They become `Message::Ctx`, or `Choice::Ctx` on an enum, a generated `<Name>Ctx` struct. An
///   enum's arms read them as fields, and pass them on with `#[with]`. The holder writes a
///   parameter, so encode cannot derive it: encode returns `EncodeError::Mismatch` when a
///   `#[count]`, `#[len]`, `#[switch]` or `#[when]` it drives disagrees with the value.
/// - `#[message(depth = N)]`: decode refuses this type nested more than `N` messages and
///   catalogues deep, counted from where decode started, with `ParseError::TooDeep { limit: N }`.
///   It sets `Message::MAX_DEPTH`, or `Choice::MAX_DEPTH` on an enum, and defaults to
///   `MAX_NESTING_DEPTH`. Each type checks its own, so a type that holds itself, such as
///   OpenWire's nested object, is bounded by the smallest limit on its cycle.
/// - `#[message(closed)]`: on an enum, an unknown discriminant fails with
///   `ParseError::Malformed`.
/// - `#[message(crate = path)]`: the path to `layline`. Default `::layline`.
///
/// # Collection attributes
///
/// A `Vec<T>` or `Box<[T]>` field takes one of these:
///
/// - `#[count(e)]`, `#[count(e, cap = N)]`: as many elements as the expression `e` gives, such
///   as `n`, `hdr.n`, `n * 2 - 1` or `rows * cols`. Decode rejects more than `N`.
/// - `#[len(e)]`, `#[len(e, cap = N)]`: as many bytes as `e` gives. Also sizes a `#[text]` string
///   or a `#[switch]` body.
/// - `#[count(prefix = T)]`, `#[len(prefix = T)]`, either with `cap = N`: the count or byte
///   length is an integer `T` immediately before the value, `u8` to `u64` or `i8` to `i64`, in the
///   message's byte order. It is no field. Encode writes it, refusing a number `T` cannot hold
///   with `EncodeError::Unfit` and one past `N` with `EncodeError::OverCap`. A prefix is the
///   number itself and takes no expression: a count of 4-byte words is a `Stated<T>` that
///   `#[len(words * 4)]` reads. `SEGMENTS` gives it a row named `<field>.count` or `<field>.len`.
/// - `#[fill]`, `#[fill(cap = N)]`: whole elements until the message ends.
/// - `#[until(b)]`: bytes up to the terminator `b`. Decode consumes it and encode appends it.
/// - `#[until(mask = M)]`: elements up to and including the first whose first byte has the `M`
///   bits set.
///
/// # Field attributes
///
/// - `#[stride(e)]`: the byte size of one element. Use with `#[count]`.
/// - `#[seek(e)]`: read at the byte offset `e` gives, counted from the start of the message. An
///   `Option<T>` at offset zero is `None`.
/// - `#[at(byte = N)]`: the byte offset where the field starts.
/// - `#[at(end)]`: the field is anchored to the end of the message, in its tail. The fields that
///   take it come last. Decode reads them first, from the end, once it has read the fields
///   before the first one that reaches the end: a `#[fill]`, a `#[switch]` or a nested message.
///   That one then stops where the tail starts, and every field may read tail fields, as the
///   tail may read the fields before it. Encode writes every field in declaration order. Each
///   tail field's size is known before decode reads it: fixed, or a `#[count]` or `#[len]` over
///   later tail fields, earlier fields and parameters, and an `Option<Vec<T>>` in the tail may be
///   sized so. A message with a tail reads to the end of the bytes it is given, and refuses a
///   byte between its other fields and its tail. RTP's padding is
///   `#[at(end)] #[when(rtp.padding)] #[count(pad_count - 1)] padding: Option<Vec<u8>>`, then
///   `#[at(end)] #[when(rtp.padding)] pad_count: Stated<u8>`; a CRC trailer is
///   `#[at(end)] #[checksum(Crc, over = head..=payload)] crc: u32`.
/// - `#[bytes(N)]`: the size of a nested layout, one element, a `#[switch]` body, or a NUL-padded
///   `#[text]` string.
/// - `#[codec(N)]`: a `FieldCodec` stored as an 8, 16, 32 or 64-bit integer.
/// - `#[var]`: a `VarCodec` value, whose size is read from the wire.
/// - `#[text]`, `#[text(Codec)]`: a `String`, as UTF-8 or through the `TextCodec` `Codec`.
/// - `#[message]`: a nested `Message`, boxed or not.
/// - `#[with(f, g)]`: earlier fields passed, in order, as the nested message's or the switch's
///   enum's `needs`: integers, `bool`s, fields of a nested layout (`hdr.f`) or parameters. Use
///   with `#[message]` or `#[switch]`. JEL's DW payload is
///   `#[switch(payload_type)] #[with(mask, num_elements, flags.big_endian)] payload: Payload`.
/// - `#[switch(e)]`: an enum whose variant the value of `e` selects. `#[switch(..)]` selects by
///   the remaining length.
/// - `#[when(c)]`: an `Option<T>` present when the condition `c` holds. `#[when(flags & MASK)]`,
///   `#[when(flag)]`, `#[when(flags & MASK == 0)]` and `#[when(!flag)]` are written: encode sets or
///   clears the bits from `Some`. Encode checks any other condition, such as
///   `#[when(has_ext && kind != 3)]`, against `Some`. `!(flags & MASK)` is refused: on an
///   integer, `!` flips the bits.
/// - `#[when(n > 0)]` or `#[when(n != 0)]` with `#[seek]`: a record present when `n`, its exact
///   byte length, is non-zero: decode refuses a record of any other length, and encode writes
///   the length. With `#[align(N)]`, `n` does not count the padding up to a multiple of `N`
///   bytes, and the record may end anywhere from `n` to that bound, as a block that states its
///   own size, padding included, does. For a number that only marks presence, use `#[seek]`
///   alone: an offset of zero is `None`.
/// - `#[align(N)]`: zero bytes after a field, up to the next multiple of `N` bytes counted from
///   the field's first byte, that its length does not count. Use with a `#[len(n)]` collection,
///   string or switch, or a `#[seek]` record with `#[when(n > 0)]`. `n` holds the field's own
///   length. Decode skips the padding without reading it, and encode writes zeros. A RIFF chunk
///   is `#[len(size)] #[align(2)] body: Vec<u8>`.
/// - `#[fill]` on an `Option<T>`: present when bytes remain.
/// - `#[present]` on an `Option<T>`: present when a byte just before it is 1, absent when it is 0.
///   Decode refuses any other byte with `ParseError::Malformed`, and encode writes it from
///   `Some`. With a prefix, the wire holds the flag, then the prefix, then the value: OpenWire's
///   nullable string is `#[present] #[len(prefix = u16)] #[text(Mutf8)] Option<String>`. An
///   `Option<Vec<T>>` or `Option<Box<[T]>>` takes `#[present]` or `#[when]` and states its own
///   size: a prefix, `#[bytes(N)]` or `#[until]`. `SEGMENTS` gives the flag a row named
///   `<field>.present`.
/// - `#[magic(0xFE)]`, `#[magic(b"SEAL")]`: fixed bytes. Decode checks them and encode writes
///   them.
/// - `#[checksum(Alg, over = a..=b)]`: a checksum over fields `a` to `b`. Decode verifies it and
///   encode computes it.
/// - `#[range(lo..=hi)]`: the allowed values. Decode rejects others, and encode refuses them with
///   `EncodeError::OutOfRange`, checking a derived length or offset once it is known.
/// - `#[valid(c)]`: a condition the whole value must satisfy, such as
///   `#[valid(rtp.payload_type == PDW)]`. It writes nothing, so it is always checked: decode
///   refuses a body that breaks it with `ParseError::Malformed`, and encode a value that does
///   with `EncodeError::Mismatch`, naming this field. It is checked after every field is read.
///
/// # Borrowed fields
///
/// A struct with one lifetime, `Name<'a>`, implements `MessageRef<'a>` instead of `Message`: its
/// `&'a [u8]` and `&'a [u8; N]` fields are views of the bytes it decodes from, so decode needs
/// no allocator. Size a `&'a [u8]` with `#[len(n)]`, `#[count(n)]`, `#[bytes(N)]` or `#[fill]`;
/// `&'a [u8; N]` is `N` bytes. Encode writes each run in one piece and states its length as for
/// a `Vec<u8>`. A borrowing message takes no `needs(..)`, and no other message can hold it.
///
/// ```
/// use layline::{Message, MessageRef};
///
/// #[derive(Message, Debug, PartialEq)]
/// pub struct Chunk<'a> {
///     pub offset: u32,
///     #[fill]
///     pub data: &'a [u8],
/// }
///
/// let wire = [0x10, 0, 0, 0, 0xAA, 0xBB];
/// let (chunk, used) = Chunk::decode(&wire).unwrap();
/// assert_eq!((chunk.offset, chunk.data, used), (0x10, &[0xAA, 0xBB][..], 6));
/// ```
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
/// - `#[with(a, b)]`: the enum's parameters, or other names an arm can read, passed in order as
///   the payload message's `needs`. Kaitai's `rfextgeo_ext(len)` is
///   `#[value(1)] #[with(len)] RfExtGeo(RfExtGeo)` in an enum that `needs(len: u16)`.
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
        when, with, present, checksum, magic, value, other, range, align, valid
    )
)]
pub fn derive_message(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match message::derive(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => refused(err, stub::message(&input)),
    }
}
