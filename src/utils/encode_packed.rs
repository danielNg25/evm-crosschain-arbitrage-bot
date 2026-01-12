use alloy::primitives::{aliases::U24, Address, U256, U8};

pub struct TakeLastXBytes(pub usize);

pub enum SolidityDataType<'a> {
    String(&'a str),
    Address(Address),
    Bytes(&'a [u8]),
    Bool(bool),
    NumberU256(U256),
    NumberU8(U8),
    NumberU24(U24),
    NumberWithShift(U256, TakeLastXBytes),
}

pub mod abi {
    use super::SolidityDataType;

    /// Pack a single `SolidityDataType` into bytes
    fn pack<'a>(data_type: &'a SolidityDataType) -> Vec<u8> {
        let mut res = Vec::new();
        match data_type {
            SolidityDataType::String(s) => {
                res.extend(s.as_bytes());
            }
            SolidityDataType::Address(a) => {
                res.extend(a.0);
            }
            SolidityDataType::NumberU256(n) => {
                res.extend(n.to_be_bytes::<32>());
            }
            SolidityDataType::NumberU8(n) => {
                res.extend(n.to_be_bytes::<1>());
            }
            SolidityDataType::NumberU24(n) => {
                res.extend(n.to_be_bytes::<3>());
            }
            SolidityDataType::Bytes(b) => {
                res.extend(*b);
            }
            SolidityDataType::Bool(b) => {
                if *b {
                    res.push(1);
                } else {
                    res.push(0);
                }
            }
            SolidityDataType::NumberWithShift(n, to_take) => {
                let local_res = n.to_be_bytes::<32>().to_vec();

                let to_skip = local_res.len() - (to_take.0 / 8);
                let local_res = local_res.into_iter().skip(to_skip).collect::<Vec<u8>>();
                res.extend(local_res);
            }
        };
        return res;
    }

    pub fn encode_packed(items: &[SolidityDataType]) -> (Vec<u8>, String) {
        let res = items.iter().fold(Vec::new(), |mut acc, i| {
            let pack = pack(i);
            acc.push(pack);
            acc
        });
        let res = res.join(&[][..]);
        let hexed = hex::encode(&res);
        (res, hexed)
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryInto;

    use alloy::primitives::address;

    use super::*;

    #[test]
    fn test_encode_packed_normal_use_case() {
        let address = hex::decode("d8b934580fcE35a11B58C6D73aDeE468a2833fa8").unwrap();
        let address: [u8; 20] = address.try_into().unwrap();
        let input = vec![
            SolidityDataType::NumberWithShift(U256::from(3838), TakeLastXBytes(24)),
            SolidityDataType::NumberU256(U256::from(4001)),
            SolidityDataType::String("this-is-a-sample-string"),
            SolidityDataType::Address(Address::from(address)),
            SolidityDataType::NumberU256(U256::from(1)),
        ];
        let (_bytes, hash) = abi::encode_packed(&input);
        let hash = format!("0x{:}", hash);
        let expected = "0x000efe0000000000000000000000000000000000000000000000000000000000000fa1746869732d69732d612d73616d706c652d737472696e67d8b934580fce35a11b58c6d73adee468a2833fa80000000000000000000000000000000000000000000000000000000000000001";
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_encode_packed_uint24() {
        let input = vec![SolidityDataType::NumberWithShift(
            U256::from(4001),
            TakeLastXBytes(24),
        )];
        let (_bytes, hash) = abi::encode_packed(&input);
        let hash = format!("0x{:}", hash);
        let expected = "0x000fa1";
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_encode_packed_uint256() {
        let input = vec![SolidityDataType::NumberU256(U256::from(3838110))];
        let (_bytes, hash) = abi::encode_packed(&input);
        let hash = format!("0x{:}", hash);
        let expected = "0x00000000000000000000000000000000000000000000000000000000003a909e";
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_encode_packed_string() {
        let input = vec![SolidityDataType::String("this-is-a-sample-string")];
        let (_bytes, hash) = abi::encode_packed(&input);
        let hash = format!("0x{:}", hash);
        let expected = "0x746869732d69732d612d73616d706c652d737472696e67";
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_encode_packed_address() {
        let address = hex::decode("d8b934580fcE35a11B58C6D73aDeE468a2833fa8").unwrap();
        let address: [u8; 20] = address.try_into().unwrap();
        let input = vec![SolidityDataType::Address(Address::from(address))];
        let (_bytes, hash) = abi::encode_packed(&input);
        let hash = format!("0x{:}", hash);
        let expected = "0xd8b934580fce35a11b58c6d73adee468a2833fa8";
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_encode_packed_bool() {
        let input = vec![SolidityDataType::Bool(false)];
        let (_bytes, hash) = abi::encode_packed(&input);
        let hash = format!("0x{:}", hash);
        let expected = "0x00";
        assert_eq!(hash, expected);
    }
    #[test]
    fn test_encode_packed_normal_bytes() {
        let bytes = "abababababababababababababababababababababababababababababab";
        let bytes = hex::decode(bytes).unwrap();
        let bytes: [u8; 30] = bytes.try_into().unwrap();

        let input = vec![SolidityDataType::Bytes(&bytes)];
        let (_bytes, hash) = abi::encode_packed(&input);
        let hash = format!("0x{:}", hash);
        let expected = "0xabababababababababababababababababababababababababababababab";
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_encode_packed_bytes_with_vec_address() {
        let addresses = vec![
            address!("0x1514000000000000000000000000000000000000"),
            address!("0x1bD56a4Eb84E3EA57EFA35526c15f8b50C17F41D"),
            address!("0xBAb93B7ad7fE8692A878B95a8e689423437cc500"),
            address!("0x9dFF955668f79a7fcB7396d244822257Fa409cD9"),
        ];
        let input = addresses
            .iter()
            .map(|a| SolidityDataType::Address(*a))
            .collect::<Vec<_>>();
        let (_bytes, hash) = abi::encode_packed(&input);
        let hash = format!("0x{:}", hash);
        let expected = "0x15140000000000000000000000000000000000001bd56a4eb84e3ea57efa35526c15f8b50c17f41dbab93b7ad7fe8692a878b95a8e689423437cc5009dff955668f79a7fcb7396d244822257fa409cd9";
        assert_eq!(hash, expected);
    }
}
