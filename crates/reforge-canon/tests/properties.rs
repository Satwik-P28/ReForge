use proptest::prelude::*;
use reforge_canon::{digest_of_canonical, digest_of_raw};
use serde_json::json;

proptest! {
    #[test]
    fn raw_digest_detects_any_byte_change(data in prop::collection::vec(any::<u8>(), 0..64), idx in 0usize..64, byte in any::<u8>()) {
        prop_assume!(!data.is_empty());
        let dig = digest_of_raw(&data);
        let mut mutated = data.clone();
        let i = idx % mutated.len();
        mutated[i] ^= byte | 1;
        prop_assume!(mutated != data);
        assert_ne!(digest_of_raw(&mutated), dig);
    }

    #[test]
    fn canonical_object_key_order_stable(a in "[a-z]{1,4}", b in "[a-z]{1,4}", v1 in -50i64..50, v2 in -50i64..50) {
        prop_assume!(a != b);
        let left = json!({ a.clone(): v1, b.clone(): v2 });
        let right = json!({ b: v2, a: v1 });
        assert_eq!(digest_of_canonical(&left).unwrap(), digest_of_canonical(&right).unwrap());
    }
}
