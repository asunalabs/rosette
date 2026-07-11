//! Every proto/ parser must reject arbitrary bytes cleanly, never panic.
//! From the eng-review test plan: "Arbitrary/corrupt bytes into every proto/
//! parser (proptest, no panics)."

use proptest::prelude::*;

proptest! {
    #[test]
    fn contact_link_from_bytes_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = proto::ContactLink::from_bytes(&bytes);
    }

    #[test]
    fn client_message_decode_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = proto::decode::<proto::ClientMessage>(&bytes);
    }

    #[test]
    fn server_message_decode_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = proto::decode::<proto::ServerMessage>(&bytes);
    }

    #[test]
    fn envelope_decode_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = proto::decode::<proto::Envelope>(&bytes);
    }

    #[test]
    fn client_frame_decode_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = proto::decode::<proto::ClientFrame>(&bytes);
    }

    #[test]
    fn server_frame_decode_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = proto::decode::<proto::ServerFrame>(&bytes);
    }
}
