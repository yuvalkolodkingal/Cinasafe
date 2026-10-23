//! Property tests (brief 13): for any size, chunk size, read offset and read
//! length, a blob reads back exactly the original bytes.

use std::io::{Read, Seek, SeekFrom};

use cinasafe_core::blob::{write_blob, BlobWriteOptions};
use cinasafe_core::{BlobId, BlobReader};
use proptest::prelude::*;

fn seal(data: &[u8], chunk: u32, pad: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let opts = BlobWriteOptions { chunk_size: chunk, pad, nonce_prefix: None };
    write_blob(&[0x11; 32], BlobId([0x22; 16]), data.len() as u64, data, &mut out, &opts).unwrap();
    out
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    #[test]
    fn reads_match(
        data in proptest::collection::vec(any::<u8>(), 0..20_000),
        chunk in 1u32..5_000,
        pad in any::<bool>(),
        reads in proptest::collection::vec((0u64..21_000, 0usize..6_000), 1..12),
        prefetch in any::<bool>(),
    ) {
        let sealed = seal(&data, chunk, pad);
        let mut r = BlobReader::open_with(sealed, &[0x11; 32], BlobId([0x22; 16]), prefetch).unwrap();
        prop_assert_eq!(r.len(), data.len() as u64);
        for (off, len) in reads {
            r.seek(SeekFrom::Start(off)).unwrap();
            let mut buf = vec![0u8; len];
            let mut got = 0;
            loop {
                let n = r.read(&mut buf[got..]).unwrap();
                if n == 0 { break; }
                got += n;
            }
            let start = (off as usize).min(data.len());
            let end = (start + len).min(data.len());
            prop_assert_eq!(&buf[..got], &data[start..end]);
        }
    }

    #[test]
    fn any_single_bit_flip_is_detected(
        data in proptest::collection::vec(any::<u8>(), 1..3_000),
        chunk in 16u32..1_024,
        flip in any::<prop::sample::Index>(),
        bit in 0u8..8,
    ) {
        let sealed = seal(&data, chunk, false);
        let mut bad = sealed.clone();
        let i = flip.index(bad.len());
        bad[i] ^= 1 << bit;
        let res = BlobReader::open_with(bad, &[0x11; 32], BlobId([0x22; 16]), false)
            .and_then(|mut r| {
                let mut out = Vec::new();
                r.read_to_end(&mut out).map_err(cinasafe_core::Error::Io)?;
                Ok(out)
            });
        prop_assert!(res.is_err(), "flip at byte {} bit {} went unnoticed", i, bit);
    }
}
