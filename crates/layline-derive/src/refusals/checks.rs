//! Compile-time checks the derives emit, and the stubs left after a refusal.

use super::*;

#[test]
fn layout_refusal_leaves_the_trait_surface_behind() {
    refused_once_with_stub(
        r#"#[derive(Layout)]
#[layout(bits = 16)]
pub struct Word {
    #[bits(4)]
    pub a: u8,
}"#,
        crate::layout::derive,
        crate::stub::layout,
        &[
            quote::quote!(impl ::layline::Layout for Word),
            quote::quote!(
                const WIRE_BYTES: ::core::primitive::usize = 0;
            ),
            quote::quote!(
                const FIELDS: &'static [::layline::table::FieldDef<'static>] = &[];
            ),
            quote::quote!(fn decode_slice),
            quote::quote!(fn encode_into),
        ],
    );
}

#[test]
fn message_refusal_leaves_the_trait_surface_behind() {
    refused_once_with_stub(
        r#"#[derive(Message)]
pub struct Packet {
    pub n: u8,
    pub items: Vec<u16>,
}"#,
        crate::message::derive,
        crate::stub::message,
        &[
            quote::quote!(impl ::layline::Message for Packet),
            quote::quote!(
                const SEGMENTS: &'static [::layline::table::SegmentDef<'static>] = &[];
            ),
            quote::quote!(fn decode_with_nested),
            quote::quote!(fn encode_into_with),
        ],
    );
}

#[test]
fn catalogue_bare_arm_is_read_as_a_message() {
    expands_with(
        r#"#[derive(Message)]
pub enum Body {
    #[value(1)]
    Point(Point),
    #[other]
    Unknown(Vec<u8>),
}"#,
        quote::quote!(<Point as ::layline::Message>::decode_with_nested),
    );
}

#[test]
fn codec_field_width_is_asserted_against_bits() {
    expands_with(
        r#"#[derive(Layout)]
#[layout(bits = 8)]
pub struct C {
    #[bits(4)]
    pub x: ThreeBits,
    #[bits(4)]
    pub rest: u8,
}"#,
        quote::quote!(
            const _: () = ::core::assert!(
                <ThreeBits as ::layline::FieldCodec>::BITS == 4u32,
                "field `x`: #[bits(4)] disagrees with `<ThreeBits as FieldCodec>::BITS`"
            );
        ),
    );
}

#[test]
fn dispatch_arm_prefix_is_asserted_against_the_catalogue() {
    expands_with(
        r#"#[derive(Dispatch)]
#[dispatch(id = u8, prefix = 3)]
pub enum Frame {
    #[value(0b101)]
    Ping(Ping),
    #[value(0b011)]
    Pong(Pong),
    #[other]
    Unknown { id: u8, body: Vec<u8> },
}"#,
        quote::quote!(
            const _: () = ::core::assert!(
                <Pong as ::layline::Layout>::PREFIX_BITS == 3,
                "variant `Pong`: `Pong` does not match `#[dispatch(prefix = 3)]`.  Write `#[layout(.., prefix = 3)]` on the payload"
            );
        ),
    );
}

#[test]
fn message_checksum_field_is_bound_to_the_algorithm_output() {
    expands_with(
        r#"#[derive(Message)]
pub struct Packet {
    pub magic: u16,
    #[checksum(wire::Crc16Ccitt, over = ..)]
    pub crc: u8,
}"#,
        quote::quote!(
            const _: () = {
                #[allow(non_snake_case)]
                fn __layline_checksum_fits_Packet_crc<
                    A: ::layline::__private::ChecksumFits<T>,
                    T,
                >() {
                }
                let _ = __layline_checksum_fits_Packet_crc::<wire::Crc16Ccitt, u8>;
            };
        ),
    );
}

#[test]
fn count_by_a_var_field_is_bound_to_wire_int() {
    expands_with(
        r#"#[derive(Message)]
pub struct Packet {
    #[var]
    pub head: Blob,
    #[count(head)]
    pub items: Vec<u8>,
}"#,
        quote::quote!(
            const _: () = {
                #[allow(non_snake_case)]
                fn __layline_carries_an_integer_head<T: ::layline::WireInt>() {}
                let _ = __layline_carries_an_integer_head::<Blob>;
            };
        ),
    );
}

#[test]
fn var_field_is_bound_to_var_codec() {
    expands_with(
        r#"#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    #[var]
    pub id: NotACodec,
}"#,
        quote::quote!(
            const _: () = {
                #[allow(non_snake_case)]
                fn __layline_is_self_delimiting_id<T: ::layline::VarCodec>() {}
                let _ = __layline_is_self_delimiting_id::<NotACodec>;
            };
        ),
    );
}

#[test]
fn element_size_is_asserted_against_wire_bytes() {
    expands_with(
        r#"#[derive(Message)]
pub struct Packet {
    pub count: u16,
    #[count(count)]
    #[bytes(8)]
    pub items: Vec<Item>,
}"#,
        quote::quote!(
            const _: () = assert!(
                <Item as ::layline::Layout>::WIRE_BYTES == 8usize,
                "field `items`: #[bytes(8)] does not match `<Item as Layout>::WIRE_BYTES`. Write the element's size"
            );
        ),
    );
}

#[test]
fn nested_reference_is_asserted_against_the_child_table() {
    expands_with(
        NESTED_COUNT,
        quote::quote!(
            const _: () = ::core::assert!(
                ::layline::table::field_width(
                    <PackedHeader as ::layline::Layout>::FIELDS,
                    "n_entires"
                ) != 0,
                "`Packet`: `header.n_entires` names no field of nested layout `PackedHeader`.  Check the spelling"
            );
        ),
    );
}

#[test]
fn nested_reference_reads_through_a_wire_int_accessor() {
    expands_with(
        NESTED_COUNT,
        quote::quote!(
            fn __layline_Packet_header_n_entires(
                __nested: &PackedHeader,
            ) -> &impl ::layline::WireInt {
                &(*__nested).n_entires
            }
        ),
    );
}

#[test]
fn nested_layout_is_bound_nestable() {
    let src = r#"#[derive(Debug, Clone, Message)]
pub struct Packet {
    #[bytes(2)]
    pub header: PackedHeader,
    #[count(header.length)]
    pub payload: Vec<u8>,
}"#;
    expands_with(
        src,
        quote::quote!(
            const _: () = {
                #[allow(non_snake_case)]
                fn __layline_is_nestable_header<T: ::layline::__private::Nestable>() {}
                let _ = __layline_is_nestable_header::<PackedHeader>;
            };
        ),
    );
    let got = expansion(src);
    let block =
        got.find("header:PackedHeader").expect("the nested layout is a field of the hidden block");
    let derives = got[..block].rfind("#[derive(").expect("the hidden block derives");
    assert!(
        got[derives..block].contains("PartialEq"),
        "the hidden block is not `PartialEq`:\n{got}"
    );
}

#[test]
fn bare_when_on_a_nested_field_is_asserted_bool() {
    let src = r#"#[derive(Debug, Clone, Message)]
pub struct Packet {
    #[bytes(1)]
    pub header: PackedHeader,
    #[when(header.flags)]
    pub timestamp: Option<u32>,
}"#;
    expands_with(
        src,
        quote::quote!(
            const _: () = assert!(
                ::layline::table::field_width(<PackedHeader as ::layline::Layout>::FIELDS, "flags")
                    == 0
                    || ::layline::table::type_is(
                        <PackedHeader as ::layline::Layout>::TYPES,
                        "flags",
                        "bool"
                    ),
                "`Packet`: field `timestamp` uses `#[when(header.flags)]`, but `flags` of `PackedHeader` is not a `bool`. Write `#[when(header.flags & 0x01)]`, or declare `flags` as `bool`"
            );
        ),
    );
    expands_with(src, quote::quote!(::layline::__private::WireFlag::to_flag));
}

#[test]
fn nested_layout_is_asserted_infallible() {
    expands_with(
        r#"#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 10, endian = be)]
struct Holder {
    #[bytes(8)]
    child: Sig,
    tail: u16,
}"#,
        quote::quote!(
            const _: () = ::core::assert!(
                !<Sig as ::layline::Layout>::DECODE_FALLIBLE,
                "field `child`: `Sig` has a fallible `decode`, so a layout cannot nest it.  Nest it in a `#[derive(Message)]`, or move its checks into this layout"
            );
        ),
    );
}

#[test]
fn an_asserting_or_internal_layout_publishes_a_fallible_decode() {
    let fallible = quote::quote!(
        const DECODE_FALLIBLE: ::core::primitive::bool = true;
    );
    expands_with(
        r#"#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 8, endian = be)]
struct Sig {
    #[magic(b"SEAL")]
    sig: [u8; 4],
    n: u32,
}"#,
        fallible.clone(),
    );
    expands_with(
        r#"#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4, endian = be, internal)]
struct Inner {
    x: u32,
}"#,
        fallible.clone(),
    );
    let plain = expansion(
        r#"#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4, endian = be)]
struct Plain {
    x: u32,
}"#,
    );
    assert!(!plain.contains(&tokens(&fallible)), "a plain record is not fallible");
}

#[test]
fn field_after_a_switch_is_asserted_bounded() {
    expands_with(
        r#"#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    #[switch(kind)]
    pub body: Body,
    pub trailer: u32,
}"#,
        quote::quote!(
            const _: () = ::core::assert!(
                !<Body as ::layline::Choice>::OPEN_ENDED,
                "`Packet`: a field follows `body`, but switch `Body` runs to the end of the body.  Put `body` last, size every arm as `[u8; N]`, or close the catalogue"
            );
        ),
    );
}

#[test]
fn an_open_ended_arm_publishes_open_ended_through_the_catalogue() {
    expands_with(
        r#"#[derive(Message)]
pub struct Greedy {
    pub head: u8,
    #[fill]
    pub rest: Vec<u8>,
}"#,
        quote::quote!(
            const OPEN_ENDED: ::core::primitive::bool = true;
        ),
    );
    expands_with(
        r#"#[derive(Message)]
#[message(closed)]
pub enum Body {
    #[value(0)]
    Bounded(u16),
    #[value(1)]
    Greedy(Greedy),
}"#,
        quote::quote!(<Greedy as ::layline::Message>::OPEN_ENDED),
    );
}

#[test]
fn switch_footprint_is_asserted_to_tile_every_arm() {
    expands_with(
        r#"#[derive(Message)]
pub struct Packet {
    pub command: u8,
    #[switch(command)]
    #[bytes(8)]
    pub args: Args,
    pub payload: u32,
}"#,
        quote::quote!(
            const _: () = ::core::assert!(
                ::layline::table::arms_fit(<Args as ::layline::Choice>::ARMS, 64),
                "`Packet`: `args` gives union `Args` 8 bytes, but an arm is not 8 bytes wide.  Declare the arm's spare bytes as a field"
            );
        ),
    );
}

/// A coding implements `BitCodec<MSB>` for one bit order only, so one bound checks both the order and the coding.
#[test]
fn a_bits_message_binds_a_var_field_to_the_end_of_the_byte_it_reads_first() {
    let header = |order: &str| {
        format!(
            r#"#[derive(Message)]
#[message(bits{order})]
pub struct Header {{
    #[bits(4)]
    pub version: u8,
    #[var]
    pub n: ExpGolomb,
}}"#
        )
    };
    expands_with(
        &header(", order = msb"),
        quote::quote!(
            const _: () = {
                #[allow(non_snake_case)]
                fn __layline_reads_its_own_bits_n<T: ::layline::BitCodec<true>>() {}
                let _ = __layline_reads_its_own_bits_n::<ExpGolomb>;
            };
        ),
    );
    expands_with(
        &header(""),
        quote::quote!(
            const _: () = {
                #[allow(non_snake_case)]
                fn __layline_reads_its_own_bits_n<T: ::layline::BitCodec<false>>() {}
                let _ = __layline_reads_its_own_bits_n::<ExpGolomb>;
            };
        ),
    );
}

#[test]
fn a_bits_message_asserts_a_codec_field_against_its_own_width() {
    expands_with(
        r#"#[derive(Message)]
#[message(bits)]
pub struct Header {
    #[bits(6)]
    pub tag: Tag,
}"#,
        quote::quote!(
            const _: () = ::core::assert!(
                <Tag as ::layline::FieldCodec>::BITS as ::core::primitive::u64 == 6,
                "Header: the width of field `tag` differs from `<Tag as FieldCodec>::BITS`.  Declare the codec's width"
            );
        ),
    );
}

#[test]
fn a_held_message_that_states_parameters_is_read_under_a_context() {
    expands_with(
        r#"#[derive(Message)]
pub struct M {
    pub n: u8,
    #[with(n)]
    #[message]
    pub child: Block,
}"#,
        quote::quote!(<Block as ::layline::Message>::Ctx::new(__b0.n)),
    );
}

#[test]
fn with_states_one_field_per_parameter_in_order() {
    expands_with(
        r#"#[derive(Message)]
pub struct M {
    pub a: u8,
    pub b: u16,
    #[with(a, b)]
    #[message]
    pub child: Block,
}"#,
        quote::quote!(<Block as ::layline::Message>::Ctx::new(self.a, self.b)),
    );
}

#[test]
fn a_held_message_with_no_with_is_read_through_message_itself() {
    expands_with(
        r#"#[derive(Message)]
pub struct M {
    pub n: u8,
    #[message]
    pub child: Block,
}"#,
        quote::quote!(<Block as ::layline::Message>::decode_with_nested),
    );
}

#[test]
fn one_payload_type_spelled_two_ways_gets_no_from() {
    let out = expansion(
        "#[derive(Dispatch)] #[dispatch(id = u8)] enum D { \
         #[value(1)] A(Sample), #[value(2)] B(self::Sample), #[other] Unknown { id: u8, body: Vec<u8> } }",
    );
    assert!(!out.contains("From<"), "{out}");
}
