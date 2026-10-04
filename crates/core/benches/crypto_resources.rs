// Synthetic fixtures only; cargo bench -p audeniq-core --bench crypto_resources.
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Generate, Payload},
};
use audeniq_core::{
    auth,
    payout_keys::{KeyRing, TAG},
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, hint::black_box};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroizing;
mod support;
use support::compare;

// The previous implementations, with the same dependencies and synthetic key.
struct Reference {
    keys: BTreeMap<i16, Zeroizing<Vec<u8>>>,
}
impl Reference {
    fn cipher(&self, version: i16) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(self.keys.get(&version).unwrap()).unwrap()
    }
    fn aad(org: Uuid, header: &[u8]) -> Vec<u8> {
        let mut aad = b"audeniq:portal.payout_accounts:account_number:".to_vec();
        aad.extend_from_slice(org.as_bytes());
        aad.extend_from_slice(header);
        aad
    }
    fn seal(&self, org: Uuid, number: &str) -> Vec<u8> {
        let mut out = TAG.to_vec();
        out.extend_from_slice(&1i16.to_be_bytes());
        let nonce = Nonce::generate();
        let ct = self
            .cipher(1)
            .encrypt(
                &nonce,
                Payload {
                    msg: number.as_bytes(),
                    aad: &Self::aad(org, &out),
                },
            )
            .unwrap();
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        out
    }
    fn open(&self, org: Uuid, sealed: &[u8]) -> Zeroizing<String> {
        let header = &sealed[..TAG.len() + 2];
        let version = i16::from_be_bytes(header[TAG.len()..].try_into().unwrap());
        let (nonce, ct) = sealed[header.len()..].split_at(12);
        let pt = Zeroizing::new(
            self.cipher(version)
                .decrypt(
                    &Nonce::try_from(nonce).unwrap(),
                    Payload {
                        msg: ct,
                        aad: &Self::aad(org, header),
                    },
                )
                .unwrap(),
        );
        Zeroizing::new(std::str::from_utf8(&pt).unwrap().to_owned())
    }
}

fn reference_secret_eq(a: &str, b: &str) -> bool {
    Sha256::digest(a.as_bytes())
        .to_vec()
        .ct_eq(&Sha256::digest(b.as_bytes()).to_vec())
        .into()
}

fn main() {
    // No runtime config/keyring is read: initialize only this standalone
    // benchmark process before it creates any threads, using synthetic keys.
    unsafe {
        std::env::set_var("PAYOUT_KMS_ENABLED", "false");
        std::env::remove_var("PAYOUT_KEYRING_FILE");
        std::env::set_var("PAYOUT_ACCOUNT_KEY", "11".repeat(32));
    }
    let new = KeyRing::from_env(false).unwrap();
    let old = Reference {
        keys: BTreeMap::from([(1, Zeroizing::new(vec![0x11; 32]))]),
    };
    let org = Uuid::from_u128(1);
    println!(
        "prepared_cipher_size_bytes={}; at most 64 key versions per keyring",
        std::mem::size_of::<Aes256Gcm>()
    );
    for text in ["11012345678901", "00000000000000000001"] {
        let a = old.seal(org, text);
        let b = new.seal(org, text).unwrap();
        assert_eq!(&*new.open(org, &a).unwrap(), text);
        assert_eq!(&*old.open(org, &b), text);
        compare(
            &format!("seal/{}", text.len()),
            10000,
            || {
                black_box(old.seal(black_box(org), black_box(text)));
            },
            || {
                black_box(new.seal(black_box(org), black_box(text)).unwrap());
            },
        );
        compare(
            &format!("open/{}", text.len()),
            10000,
            || {
                black_box(old.open(black_box(org), black_box(&a)));
            },
            || {
                black_box(new.open(black_box(org), black_box(&a)).unwrap());
            },
        );
    }
    let secret = "synthetic-service-secret-at-least-32-characters";
    for (label, provided) in [
        ("match", secret),
        ("mismatch", "synthetic-different-secret"),
    ] {
        assert_eq!(
            reference_secret_eq(provided, secret),
            auth::secret_eq(provided, secret)
        );
        compare(
            &format!("secret/{label}"),
            100000,
            || {
                black_box(reference_secret_eq(black_box(provided), black_box(secret)));
            },
            || {
                black_box(auth::secret_eq(black_box(provided), black_box(secret)));
            },
        );
    }
    println!(
        "The API additionally prehashes its configured service secret once per AppState; Argon2 cost parameters are unchanged."
    );
    println!(
        "Payload counters use System, exclude warm key state and are not process RSS; dev-profile timings are not production release timings."
    );
}
