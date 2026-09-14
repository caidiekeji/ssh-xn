//! 凭据加密（F1.1 / N7）：AES-256-GCM，密钥派生自设备指纹。
//! 指纹来源优先级：/etc/machine-id → /var/lib/dbus/machine-id → hostname。
//! 派生结果用固定盐 + SHA-256，保证重启后同一设备可解密。

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// 固定盐，随应用版本一起冻结（换盐会导致旧数据无法解密，需迁移）。
const KEY_SALT: &[u8] = b"ai-ssh:v1:device-key";

/// 读取设备指纹（不依赖任何第三方库）。
pub fn device_fingerprint() -> String {
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(s) = std::fs::read_to_string(path) {
            let s = s.trim();
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    // 沙箱/非常规环境兜底：hostname
    std::env::var("HOSTNAME").unwrap_or_else(|_| "ai-ssh-default-device".into())
}

/// 从设备指纹派生 32 字节密钥。
pub fn derive_key(fingerprint: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(KEY_SALT);
    hasher.update(fingerprint.as_bytes());
    hasher.finalize().into()
}

/// 加密：返回 [12 字节 nonce || ciphertext]。
pub fn encrypt(plain: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plain)
        .map_err(|_| Error::Crypto("加密失败".into()))?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// 解密：输入 [12 字节 nonce || ciphertext]。
pub fn decrypt(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    if data.len() < 13 {
        return Err(Error::Crypto("密文过短".into()));
    }
    let (nonce_bytes, ct) = data.split_at(12);
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ct)
        .map_err(|_| Error::Crypto("解密失败：密钥不匹配或数据损坏".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let fp = "test-device-001";
        let key = derive_key(fp);
        let plain = b"SuperSecretPassword!";
        let enc = encrypt(plain, &key).unwrap();
        assert_ne!(&enc[12..], plain);
        let dec = decrypt(&enc, &key).unwrap();
        assert_eq!(dec, plain);
    }

    #[test]
    fn wrong_key_fails() {
        let key_a = derive_key("device-a");
        let key_b = derive_key("device-b");
        let enc = encrypt(b"secret", &key_a).unwrap();
        assert!(decrypt(&enc, &key_b).is_err());
    }
}
