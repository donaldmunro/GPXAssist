use std::{error::Error, path::PathBuf};

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce
};
use chrono::Duration;
use hex;

type EncryptedData = Vec<u8>;

const KEY: &str = "b93597749e7e4c5eac98b14c8530d788b93597749e7e4c5eac98b14c8530d788";

pub fn encrypt(password: &str) -> Result<EncryptedData, aes_gcm::Error>
//-----------------------------------------------------------------------------------------------
{
   // let key = Aes256Gcm::generate_key(&mut OsRng);
   let key_bytes = hex::decode(KEY).expect("Invalid hex key");
   let cipher = Aes256Gcm::new_from_slice(&key_bytes).expect("Invalid key length");
   let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

   let mut ciphertext = cipher.encrypt(&nonce, password.as_bytes())?;
   let mut result = Vec::with_capacity(nonce.len() + ciphertext.len());
   result.extend_from_slice(&nonce);
   result.append(&mut ciphertext);

   Ok(result)
}

pub fn decrypt(data: &[u8]) -> Result<String, Box<dyn Error>>
//---------------------------------------------------------------------------------------
{
   // let key = Aes256Gcm::generate_key(&mut OsRng);
   let key_bytes = hex::decode(KEY).expect("Invalid hex key");
   const NONCE_LEN: usize = 12; // GCM nonce size

   if data.len() < NONCE_LEN
   {
      return Err("Encrypted data too short".into());
   }

   let cipher = Aes256Gcm::new_from_slice(&key_bytes).expect("Invalid key length");
   let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
   let nonce = Nonce::clone_from_slice(nonce_bytes);

   let plaintext = cipher.decrypt(&nonce, ciphertext).map_err(|e| format!("Decryption failed: {:?}", e))?;
   Ok(String::from_utf8(plaintext)?)
}

pub fn get_file_age(path: &PathBuf) -> Result<Duration, Box<dyn Error>>
//---------------------------------------------------------------------------------------
{
   let metadata = std::fs::metadata(path)?;
   let modified_time = metadata.modified()?;
   let duration_since_modified = modified_time.elapsed()?;
   let chrono_duration = Duration::from_std(duration_since_modified)?;
   Ok(chrono_duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let password = "my_secret_password";
        let encrypted = encrypt(password).expect("Encryption failed");
        let decrypted = decrypt(&encrypted).expect("Decryption failed");
        assert_eq!(password, decrypted);
    }

    #[test]
    fn test_decrypt_invalid_data() {
        // Create data that is long enough (nonce + ciphertext) but invalid
        // 12 bytes nonce + some ciphertext
        let mut data = vec![0u8; 20];
        // Fill with some random values to ensure it's not a valid tag/ciphertext
        for i in 0..data.len() {
            data[i] = (i % 255) as u8;
        }

        let result = decrypt(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_short_data() {
        let data = vec![0u8; 5]; // Too short for nonce (12 bytes)
        let result = decrypt(&data);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Encrypted data too short");
    }

    #[test]
    fn test_encrypt_produces_different_outputs() {
        let password = "password";
        let enc1 = encrypt(password).expect("Encryption failed");
        let enc2 = encrypt(password).expect("Encryption failed");
        // Because of the random nonce, outputs should be different even for same input
        assert_ne!(enc1, enc2);
    }
}
