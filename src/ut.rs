use std::{error::Error, path::PathBuf};
use std::env;

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce
};
use hex;

type EncryptedData = Vec<u8>;

const KEY: &str = "b93597749e7e4c5eac98b14c8530d788b93597749e7e4c5eac98b14c8530d788";

pub fn encrypt(password: &str) -> Result<EncryptedData, aes_gcm::Error> 
//-----------------------------------------------------------------------------------------------
{
   // let key = Aes256Gcm::generate_key(&mut OsRng);
   let key_bytes = hex::decode(KEY).expect("Invalid hex key");
   let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
   let cipher = Aes256Gcm::new(key);
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
   let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
   const NONCE_LEN: usize = 12; // GCM nonce size
    
   if data.len() < NONCE_LEN 
   {
      return Err("Encrypted data too short".into());
   }

   let cipher = Aes256Gcm::new(key);
   let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
   let nonce = Nonce::from_slice(nonce_bytes);
    
   let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|e| format!("Decryption failed: {:?}", e))?;
   Ok(String::from_utf8(plaintext)?)
}
