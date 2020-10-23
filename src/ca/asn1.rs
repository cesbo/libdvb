const SIZE_INDICATOR: u8 = 0x80;


pub fn encode(value: u16, out: &mut Vec<u8>) {
    if value < SIZE_INDICATOR as _ {
        out.push(value as u8);
    } else if value < 0x100 {
        out.push(SIZE_INDICATOR + 1);
        out.push(value as u8);
    } else {
        out.push(SIZE_INDICATOR + 2);
        out.push((value >> 8) as u8);
        out.push(value as u8);
    }
}
