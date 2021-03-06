use bitwrap::*;


#[test]
fn test_readme_skip() {
    #[derive(Default, BitWrap)]
    struct Packet {
        #[bits(6)] f1: u8,
        #[bits(2, skip = 0)]
        #[bits(4, skip = 0b1111)]
        #[bits(4)] f2: u8,
    }

    const DATA: &[u8] = &[0xAC, 0xF5];

    let mut packet = Packet::default();
    let result = packet.unpack(DATA).unwrap();

    assert_eq!(result, DATA.len());
    assert_eq!(packet.f1, 0x2B);
    assert_eq!(packet.f2, 0x05);

    let mut buffer: [u8; 2] = [0; 2];
    let result = packet.pack(&mut buffer).unwrap();

    assert_eq!(result, DATA.len());
    assert_eq!(&buffer[.. result], DATA);
}
