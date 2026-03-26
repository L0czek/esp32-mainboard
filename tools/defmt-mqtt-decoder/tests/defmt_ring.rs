#[path = "../../../src/defmt_ring.rs"]
mod defmt_ring;

use defmt_ring::DefmtRing;

#[test]
fn shared_ring_preserves_fifo_order() {
    let mut ring = DefmtRing::<8>::new();
    assert_eq!(ring.push_slice(b"abcd"), 4);

    let mut out = [0u8; 4];
    assert_eq!(ring.pop_into(&mut out), 4);
    assert_eq!(&out, b"abcd");
}

#[test]
fn shared_ring_partial_drain_keeps_tail() {
    let mut ring = DefmtRing::<8>::new();
    assert_eq!(ring.push_slice(b"abcdef"), 6);

    let mut first = [0u8; 3];
    assert_eq!(ring.pop_into(&mut first), 3);
    assert_eq!(&first, b"abc");

    let mut second = [0u8; 3];
    assert_eq!(ring.pop_into(&mut second), 3);
    assert_eq!(&second, b"def");
}
