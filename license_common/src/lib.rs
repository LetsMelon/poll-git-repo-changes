use chrono::NaiveDateTime;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey, rand_core::CryptoRngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct License {
    id: String,
    server_id: String,
    valid_from: NaiveDateTime,
    valid_until: NaiveDateTime,
}

impl License {
    pub fn new(
        id: String,
        server_id: String,
        valid_from: NaiveDateTime,
        valid_until: NaiveDateTime,
    ) -> Self {
        Self {
            id,
            server_id,
            valid_from,
            valid_until,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    content: License,
    encrypted_hash: Vec<u8>,
}

impl Message {
    pub fn new<R: CryptoRngCore>(content: License, pub_key: &RsaPublicKey, rng: &mut R) -> Self {
        let raw_license = postcard::to_allocvec(&content).unwrap();
        let hash = Sha256::digest(&raw_license);

        let encrypted_hash = pub_key.encrypt(rng, Pkcs1v15Encrypt, &hash[..]).unwrap();

        Self {
            content,
            encrypted_hash,
        }
    }

    pub fn get_content(&self) -> &License {
        &self.content
    }

    pub fn encrypt(&self) -> Result<Vec<u8>, ()> {
        Ok(postcard::to_allocvec(&self).unwrap())
    }

    pub fn decrypt<R: CryptoRngCore>(
        data: &[u8],
        pub_key: &RsaPublicKey,
        rng: &mut R,
    ) -> Result<Self, ()> {
        let sth: Message = postcard::from_bytes(data).unwrap();
        let other_message = Self::new(sth.content.clone(), pub_key, rng);

        if !sth.encrypted_hash.eq(&other_message.encrypted_hash) {
            return Err(());
        }

        Ok(sth)
    }
}
