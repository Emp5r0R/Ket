//! Minimal deterministic CBOR: unsigned integers, byte and text strings,
//! arrays, and `null`, each in its shortest definite-length form.

use crate::InviteError;

const MAJOR_UINT: u8 = 0;
const MAJOR_BYTES: u8 = 2;
const MAJOR_TEXT: u8 = 3;
const MAJOR_ARRAY: u8 = 4;
const NULL: u8 = 0xf6;

pub(crate) struct Decoder<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    pub(crate) fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    pub(crate) fn finish(self) -> Result<(), InviteError> {
        (self.offset == self.input.len())
            .then_some(())
            .ok_or(InviteError::Invalid)
    }

    pub(crate) fn array(&mut self, expected: usize) -> Result<(), InviteError> {
        (self.array_len()? == expected)
            .then_some(())
            .ok_or(InviteError::Invalid)
    }

    pub(crate) fn array_len(&mut self) -> Result<usize, InviteError> {
        self.length(MAJOR_ARRAY)
    }

    pub(crate) fn uint(&mut self) -> Result<u64, InviteError> {
        let byte = self.byte()?;
        if byte >> 5 != MAJOR_UINT {
            return Err(InviteError::Invalid);
        }
        self.argument(byte & 0x1f)
    }

    pub(crate) fn optional_uint(&mut self) -> Result<Option<u64>, InviteError> {
        if self.input.get(self.offset) == Some(&NULL) {
            self.offset += 1;
            return Ok(None);
        }
        self.uint().map(Some)
    }

    pub(crate) fn bytes(&mut self, max: usize) -> Result<&'a [u8], InviteError> {
        let length = self.length(MAJOR_BYTES)?;
        self.take(length, max)
    }

    pub(crate) fn text(&mut self, max: usize) -> Result<&'a str, InviteError> {
        let length = self.length(MAJOR_TEXT)?;
        std::str::from_utf8(self.take(length, max)?).map_err(|_| InviteError::Invalid)
    }

    fn take(&mut self, length: usize, max: usize) -> Result<&'a [u8], InviteError> {
        if length > max {
            return Err(InviteError::TooLarge);
        }
        let end = self
            .offset
            .checked_add(length)
            .filter(|end| *end <= self.input.len())
            .ok_or(InviteError::Invalid)?;
        let value = &self.input[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    fn length(&mut self, major: u8) -> Result<usize, InviteError> {
        let byte = self.byte()?;
        if byte >> 5 != major {
            return Err(InviteError::Invalid);
        }
        usize::try_from(self.argument(byte & 0x1f)?).map_err(|_| InviteError::TooLarge)
    }

    /// Decodes an argument and rejects any form longer than the shortest one.
    fn argument(&mut self, additional: u8) -> Result<u64, InviteError> {
        let (value, minimum) = match additional {
            value @ 0..=23 => return Ok(value.into()),
            24 => (u64::from(self.byte()?), 24),
            25 => (u64::from(u16::from_be_bytes(self.fixed()?)), 0x100),
            26 => (u64::from(u32::from_be_bytes(self.fixed()?)), 0x1_0000),
            27 => (u64::from_be_bytes(self.fixed()?), 0x1_0000_0000),
            _ => return Err(InviteError::Invalid),
        };
        (value >= minimum)
            .then_some(value)
            .ok_or(InviteError::Invalid)
    }

    fn byte(&mut self) -> Result<u8, InviteError> {
        let value = *self.input.get(self.offset).ok_or(InviteError::Invalid)?;
        self.offset += 1;
        Ok(value)
    }

    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], InviteError> {
        let slice = self.take(N, N)?;
        let mut value = [0_u8; N];
        value.copy_from_slice(slice);
        Ok(value)
    }
}

pub(crate) fn encode_array(output: &mut Vec<u8>, length: usize) {
    encode_head(output, MAJOR_ARRAY, length as u64);
}

pub(crate) fn encode_uint(output: &mut Vec<u8>, value: u64) {
    encode_head(output, MAJOR_UINT, value);
}

pub(crate) fn encode_optional_uint(output: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => encode_uint(output, value),
        None => output.push(NULL),
    }
}

pub(crate) fn encode_bytes(output: &mut Vec<u8>, value: &[u8]) {
    encode_head(output, MAJOR_BYTES, value.len() as u64);
    output.extend_from_slice(value);
}

pub(crate) fn encode_text(output: &mut Vec<u8>, value: &str) {
    encode_head(output, MAJOR_TEXT, value.len() as u64);
    output.extend_from_slice(value.as_bytes());
}

fn encode_head(output: &mut Vec<u8>, major: u8, value: u64) {
    let prefix = major << 5;
    match value {
        0..=23 => output.push(prefix | value as u8),
        24..=0xff => output.extend_from_slice(&[prefix | 24, value as u8]),
        0x100..=0xffff => {
            output.push(prefix | 25);
            output.extend_from_slice(&(value as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            output.push(prefix | 26);
            output.extend_from_slice(&(value as u32).to_be_bytes());
        }
        _ => {
            output.push(prefix | 27);
            output.extend_from_slice(&value.to_be_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded_uint(value: u64) -> Vec<u8> {
        let mut output = Vec::new();
        encode_uint(&mut output, value);
        output
    }

    #[test]
    fn integers_round_trip_at_every_width_boundary() {
        for value in [
            0,
            23,
            24,
            0xff,
            0x100,
            0xffff,
            0x1_0000,
            0xffff_ffff,
            0x1_0000_0000,
            u64::MAX,
        ] {
            let encoded = encoded_uint(value);
            let mut decoder = Decoder::new(&encoded);
            assert_eq!(decoder.uint().unwrap(), value);
            decoder.finish().unwrap();
        }
    }

    #[test]
    fn non_shortest_and_unsupported_forms_are_rejected() {
        for input in [
            &[0x18, 0x17][..],               // 23 in the one-byte form
            &[0x19, 0x00, 0xff],             // 255 in the two-byte form
            &[0x1a, 0x00, 0x00, 0xff, 0xff], // 65535 in the four-byte form
            &[0x1b, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff],
            &[0x1c],       // reserved additional info
            &[0x1f],       // indefinite length
            &[0x20],       // negative integer
            &[0x19, 0x01], // truncated
        ] {
            assert!(Decoder::new(input).uint().is_err(), "{input:02x?}");
        }
    }

    #[test]
    fn strings_are_bounded_typed_and_complete() {
        let mut output = Vec::new();
        encode_text(&mut output, "ket");
        encode_bytes(&mut output, &[1, 2, 3]);
        let mut decoder = Decoder::new(&output);
        assert_eq!(decoder.text(3).unwrap(), "ket");
        assert_eq!(decoder.bytes(3).unwrap(), &[1, 2, 3]);
        decoder.finish().unwrap();

        assert_eq!(Decoder::new(&output).text(2), Err(InviteError::TooLarge));
        assert_eq!(Decoder::new(&output).bytes(8), Err(InviteError::Invalid));
        assert_eq!(
            Decoder::new(&[0x63, b'k']).text(8),
            Err(InviteError::Invalid)
        );
        assert_eq!(
            Decoder::new(&[0x62, 0xc3, 0x28]).text(8),
            Err(InviteError::Invalid)
        );
        let mut trailing = Decoder::new(&[0x01, 0x02]);
        trailing.uint().unwrap();
        assert_eq!(trailing.finish(), Err(InviteError::Invalid));
    }
}
