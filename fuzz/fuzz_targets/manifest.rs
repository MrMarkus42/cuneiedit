//! Reading arbitrary update information as downloaded from the network.
#![no_main]

use libfuzzer_sys::fuzz_target;
use cuneiedit_update::{Manifest, Version, verified_manifest, verify};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Some(manifest) = Manifest::parse(&text) {
        assert_eq!(Manifest::parse(&manifest.to_text()), Some(manifest));
    }
    Version::parse(&text);
    // A signature is the second half of the input; no random input may pass.
    let (message, signature) = data.split_at(data.len() / 2);
    let signature = String::from_utf8_lossy(signature);
    assert!(!verify(message, &signature));
    assert!(verified_manifest(message, &signature).is_err());
});
