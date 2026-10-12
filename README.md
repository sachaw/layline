# layline

Binary wire formats as Rust types.

Derive `Layout` for fixed-size records and `Message` for variable-length ones.
Fields must cover every bit, or the build fails. Encode fills in counts, lengths
and checksums; decode checks them.

```toml
[dependencies]
layline = "0.1"
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
assert_eq!(header.encode(), [0x1A, 0x05]);
```

A layout of up to 64 bits is shifts and masks on one integer. At 8, 16, 32 or 64 bits it also
converts to and from that integer: `Header::from_bits(0x1A05)` is the header above, read
big-endian. Add `truncate` to `#[layout(..)]` to have encode keep the low bits of an oversized
value, as a hardware register does, instead of panicking.

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
    pub len: u8,
    #[count(len)] pub payload: Vec<u8>,
    #[checksum(Crc16, over = len..)] pub crc: u16,
}

let frame = Frame { start: 0, len: 0, payload: vec![1, 2, 3], crc: 0 };
let wire = frame.encode();
assert_eq!(wire[..5], [0xFE, 3, 1, 2, 3]);

let (back, used) = Frame::decode(&wire).unwrap();
assert_eq!((back.payload, used), (vec![1, 2, 3], wire.len()));
```

`#[align(N)]` after a `#[len(n)]` field adds zero padding up to a multiple of `N` bytes that `n`
does not count, as a RIFF chunk's odd body has.

A field can be present when a flag is clear: `#[when(flags & 0x4 == 0)]` or `#[when(!flag)]`.
Encode writes the flag from the field, setting the bits for `None`.

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

Every attribute is documented on its derive.

## Features

| Feature | Enables | Default |
|---|---|---|
| `derive` | The derive macros | Yes |
| `alloc` | `Vec`, `String` and `Box` fields | Yes |
| `frame` | Stream framing | No |
| `num` | LEB128, Exp-Golomb and reserved values | No |
| `serde` | `Serialize` for `U`, `I` and `Reserved` | No |

## Crates

- `layline-core`: the `no_std` runtime, with no dependencies.
- `layline-derive`: the derive macros.
- `layline-codegen`: generates Rust source from a model, for formats defined outside Rust.

## License

MPL-2.0
