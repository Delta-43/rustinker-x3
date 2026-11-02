//! TLS 1.3 for the internet route (ADR 026). The server certificate is NOT
//! checked: authenticity comes from the signed requests and replies in
//! `x3_proto::sign`, so a man-in-the-middle can read what is fetched but cannot
//! forge it. This file must never be used to send a secret.
use embedded_tls::{
    Aes128GcmSha256, CertificateRef, CertificateVerifyRef, CryptoProvider, CryptoRngCore,
    TlsCipherSuite, TlsError, TlsVerifier,
};

/// esp-hal's `Rng` is a true random source while the radio is on (it samples
/// radio noise), but the crate marks only its `Trng` as cryptographic, and
/// `Trng` needs the ADC the buttons use. So this wrapper is only sound while
/// Wi-Fi is up, which is the only time TLS runs.
pub struct RadioRng(pub esp_hal::rng::Rng);
impl Default for RadioRng {
    fn default() -> Self {
        Self(esp_hal::rng::Rng::new())
    }
}
impl rand_core::RngCore for RadioRng {
    fn next_u32(&mut self) -> u32 {
        self.0.random()
    }
    fn next_u64(&mut self) -> u64 {
        (self.0.random() as u64) << 32 | self.0.random() as u64
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.read(dest);
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.0.read(dest);
        Ok(())
    }
}
impl rand_core::CryptoRng for RadioRng {}

/// Accepts any certificate. See the file comment.
#[derive(Default)]
pub struct AcceptAny;
impl TlsVerifier<Aes128GcmSha256> for AcceptAny {
    fn set_hostname_verification(&mut self, _hostname: &str) -> Result<(), TlsError> {
        Ok(())
    }
    fn verify_certificate(
        &mut self,
        _transcript: &<Aes128GcmSha256 as TlsCipherSuite>::Hash,
        _cert: CertificateRef,
    ) -> Result<(), TlsError> {
        Ok(())
    }
    fn verify_signature(&mut self, _verify: CertificateVerifyRef) -> Result<(), TlsError> {
        Ok(())
    }
}

#[derive(Default)]
pub struct Unchecked {
    rng: RadioRng,
    verifier: AcceptAny,
}
impl CryptoProvider for Unchecked {
    type CipherSuite = Aes128GcmSha256;
    type Signature = [u8; 0];
    fn rng(&mut self) -> impl CryptoRngCore {
        &mut self.rng
    }
    fn verifier(&mut self) -> Result<&mut impl TlsVerifier<Aes128GcmSha256>, TlsError> {
        Ok(&mut self.verifier)
    }
}
