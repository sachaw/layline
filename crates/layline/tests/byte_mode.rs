//! Byte mode over an 80-byte C-struct-shaped telemetry record.

#![cfg(feature = "derive")]
#[path = "support/fixtures.rs"]
mod fixtures;

use fixtures::NavState;
use layline::Layout;

#[test]
fn solved_offsets_match_the_c_struct_layout() {
    let expected: &[(&str, u32)] = &[
        ("week", 0),
        ("time_of_week", 4 * 8),
        ("nav_status", 12 * 8),
        ("hw_status", 16 * 8),
        ("theta[0]", 20 * 8),
        ("theta[1]", 24 * 8),
        ("theta[2]", 28 * 8),
        ("uvw[0]", 32 * 8),
        ("uvw[1]", 36 * 8),
        ("uvw[2]", 40 * 8),
        ("lla[0]", 44 * 8),
        ("lla[1]", 52 * 8),
        ("lla[2]", 60 * 8),
        ("ned[0]", 68 * 8),
        ("ned[1]", 72 * 8),
        ("ned[2]", 76 * 8),
    ];
    assert_eq!(NavState::FIELDS.len(), expected.len());
    for (def, &(name, start)) in NavState::FIELDS.iter().zip(expected) {
        assert_eq!(def.name, name);
        assert_eq!(def.extent.start(), start as u64, "field `{name}`");
    }
    assert_eq!(
        layline::table::check_layout(NavState::FIELDS, (NavState::WIRE_BYTES * 8) as u64),
        Ok(())
    );
}

#[test]
fn a_semantically_real_sample_round_trips() {
    let ins = NavState {
        week: 2374,
        time_of_week: 345_601.8,
        nav_status: 0x0800_0000,
        hw_status: 0,
        theta: [0.01, -0.02, 1.57],
        uvw: [12.5, -0.3, 0.1],
        lla: [-33.8688, 151.2093, 58.0],
        ned: [1.0, 2.0, -3.0],
    };
    assert_eq!(NavState::decode(&ins.encode()), ins);
}
