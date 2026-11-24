# layline

Binary wire formats as Rust types.

Derive `Layout` for fixed-size records and `Message` for variable-length ones.
Fields must cover every bit, or the build fails. Encode fills in counts, lengths
and checksums; decode checks them.

Encode is as strict as decode. A type that declares something a value can break, such as a field
narrower than its type, a `#[range]`, a cap or a count field too narrow for its collection,
returns `Result<_, EncodeError>` from `encode`, naming the field, what it breaks and the value. A
type that declares nothing of the kind, and holds no type that does, returns the bytes.

```toml
[dependencies]
layline = "0.6"
```

## Layouts

```rust
use layline::Layout;

#[derive(Layout, Debug, PartialEq)]
#[layout(bits = 16, endian = be, order = msb)]
pub struct Header {
    #[bits(4)] pub version: u8,
    #[bits(1)] pub urgent: bool,
    #[bits(11)] pub length: u16,
}

let header = Header::decode(&[0x1A, 0x05]);
assert_eq!((header.version, header.urgent, header.length), (1, true, 0x205));
assert_eq!(header.encode(), Ok([0x1A, 0x05]));

// Four bits cannot hold 16.
let wide = Header { version: 16, ..header };
assert_eq!(
    wide.encode(),
    Err(layline::EncodeError::Unfit { field: "version", value: 16, bits: 4 }),
);
```

A layout of up to 64 bits is shifts and masks on one integer. At 8, 16, 32 or 64 bits it also
converts to and from that integer: `Header::from_bits(0x1A05)` is the header above, read
big-endian. Add `truncate` to `#[layout(..)]` to have encode keep the low bits of an oversized
value, as a hardware register does, instead of refusing it; such a layout's `encode` returns the
bytes.

Add `view` to `#[layout(..)]` for `<Name>View`, which reads and writes fields in place. A view
reads any bytes, so a reader can inspect a corrupt record; `verify()` runs the checks `decode`
runs, `checked(&wire)` views only bytes that pass them, and `seal()` restamps magic bytes and
checksums after the setters.

In a `bytes = N` layout a `bool` is one byte that must be 0 or 1: decode refuses any other value.
A flag byte that any nonzero value sets is `#[codec(8)] halted: layline::Nonzero`.

## Messages

```rust
use layline::Message;
use layline::checksum::Crc;

type Crc16 = Crc<u16, 0x1021, 0xFFFF, 0x0000, false>;

#[derive(Message, Debug, PartialEq)]
pub struct Frame {
    #[magic(0xFE)] pub start: u8,
    #[count(prefix = u8)] pub payload: Vec<u8>,
    #[checksum(Crc16, over = payload..)] pub crc: u16,
}

let frame = Frame { start: 0, payload: vec![1, 2, 3], crc: 0 };
let wire = frame.encode().unwrap();
assert_eq!(wire[..5], [0xFE, 3, 1, 2, 3]);

// A `u8` length cannot count 256 bytes.
let long = Frame { payload: vec![0; 256], ..frame };
assert!(long.encode().is_err());

let (back, used) = Frame::decode(&wire).unwrap();
assert_eq!((back.payload, used), (vec![1, 2, 3], wire.len()));
```

A length, count or presence flag just before its value is no field: `#[count(prefix = u8)]`,
`#[len(prefix = u16)]` and `#[present]` say where it is. One elsewhere is a `Stated<T>` field in
its place in wire order, which an attribute such as `#[count(n)]` writes. Encode writes it,
decode reads it, and equality ignores it, so a decoded value equals the one built.

```rust
use layline::{Message, Stated};

#[derive(Message, Debug, PartialEq, Default)]
pub struct Batch {
    pub n: Stated<u8>,
    pub tag: u16,
    #[count(n)] pub items: Vec<u16>,
}

let batch = Batch { tag: 7, items: vec![1, 2], ..Batch::default() };
let wire = batch.encode().unwrap();
assert_eq!(wire, [2, 7, 0, 1, 0, 2, 0]);
assert_eq!(Batch::decode(&wire).unwrap().0, batch);
```

`#[align(N)]` after a `#[len(n)]` field adds zero padding up to a multiple of `N` bytes that `n`
does not count, as a RIFF chunk's odd body has.

A field can be present when a flag is clear: `#[when(flags & 0x4 == 0)]` or `#[when(!flag)]`.
Encode writes the flag from the field, setting the bits for `None`.

A message can take its byte order from its holder, picked once per message:

```rust
use layline::Message;

#[derive(Message, Debug, PartialEq)]
#[message(needs(big: bool), endian = needs(big))]
pub struct Sample {
    pub value: u16,
}

#[derive(Message, Debug, PartialEq)]
pub struct Packet {
    pub big: bool,
    #[with(big)]
    #[message]
    pub sample: Sample,
}

let wire = Packet { big: true, sample: Sample { value: 0x0102 } }.encode();
assert_eq!(wire, [1, 0x01, 0x02]);
let wire = Packet { big: false, sample: Sample { value: 0x0102 } }.encode();
assert_eq!(wire, [0, 0x02, 0x01]);
```

Fields anchored to the end with `#[at(end)]` form the message's tail. Decode reads them first,
from the end, so a `#[fill]` stops where they start. RTP's padding counts itself in the last
byte:

```rust
use layline::{Message, Stated};

#[derive(Message, Debug, PartialEq)]
pub struct Rtp {
    pub padded: bool,
    #[fill]
    pub payload: Vec<u8>,
    #[at(end)] #[when(padded)] #[count(pad_count - 1)]
    pub padding: Option<Vec<u8>>,
    #[at(end)] #[when(padded)]
    pub pad_count: Stated<u8>,
}

let rtp = Rtp { padded: true, payload: vec![7], padding: Some(vec![0, 0]), pad_count: Stated::default() };
let wire = rtp.encode().unwrap();
assert_eq!(wire, [1, 7, 0, 0, 3]);
assert_eq!(Rtp::decode(&wire).unwrap().0, rtp);
```

A message may hold itself, through `Box`, `Vec` or other types, as a BER element or an OpenWire
object does. `#[message(depth = N)]` bounds how deeply decode nests it.

A struct with a lifetime borrows its byte runs from the input instead of copying them, so it
decodes without an allocator. It implements `MessageRef<'a>`:

```rust
use layline::{Message, MessageRef};

#[derive(Message, Debug, PartialEq)]
pub struct Chunk<'a> {
    pub offset: u32,
    #[fill]
    pub data: &'a [u8],
}

let wire = [0x10, 0, 0, 0, 0xAA, 0xBB];
let (chunk, _) = Chunk::decode(&wire).unwrap();
assert_eq!(chunk.data, &[0xAA, 0xBB]);
```

## Switches

```rust
use layline::{Layout, Message};

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 4)]
pub struct Ping { pub seq: u32 }

#[derive(Message, Debug, Clone, PartialEq)]
pub enum Body {
    #[value(1)] #[bytes(4)] Ping(Ping),
    #[other] Unknown(Vec<u8>),
}

#[derive(Message, Debug, PartialEq)]
pub struct Packet {
    pub kind: u8,
    #[switch(kind)] pub body: Body,
}

let (packet, _) = Packet::decode(&[1, 7, 0, 0, 0]).unwrap();
assert_eq!(packet.body, Body::Ping(Ping { seq: 7 }));
```

A selector is an expression, as every attribute's value is, and may read several fields:
`#[switch(if head.marker || head.pt == 127 { DESCRIBED } else { head.pt })]`. Encode checks such a
selector against the arm, and writes the field of one it can work back to.

A switch passes its holder's fields to the arm it selects with `#[with(..)]`, as a nested
message takes them: the enum declares `#[message(needs(..))]`, and an arm reads them, or passes
them to its own message with `#[with(..)]`. Each value is an expression in the holder's
fields, such as Kaitai's `#[with(extension_len - 1 - len)]`, which encode reads and never writes.

Every attribute is documented on its derive.

## Features

| Feature | Enables | Default |
|---|---|---|
| `derive` | The derive macros | Yes |
| `alloc` | `Vec`, `String` and `Box` fields | Yes |
| `frame` | Stream framing | No |
| `num` | LEB128, VLQ, Exp-Golomb and reserved values | No |
| `serde` | `Serialize` for `U`, `I` and `Reserved` | No |

## Crates

- `layline-core`: the `no_std` runtime, with no dependencies.
- `layline-derive`: the derive macros.
- `layline-codegen`: generates Rust source from a model, for formats defined outside Rust.

## License

MPL-2.0
