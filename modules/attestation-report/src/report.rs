use crate::errors::AttestationReportError as Error;
#[cfg(feature = "sgx")]
use crate::sgx_reexport_prelude::*;
use chrono::prelude::DateTime;
use core::fmt::Debug;
use crypto::Address;
use lcp_types::Time;
use pem;
use serde::{Deserialize, Serialize};
use sgx_types::sgx_quote_t;
use std::string::String;
use std::vec::Vec;
use std::{format, ptr};
use tendermint::Time as TmTime;

pub const IAS_REPORT_CA: &[u8] =
    include_bytes!("../../../enclave/Intel_SGX_Attestation_RootCA.pem");

type SignatureAlgorithms = &'static [&'static webpki::SignatureAlgorithm];
static SUPPORTED_SIG_ALGS: SignatureAlgorithms = &[
    &webpki::ECDSA_P256_SHA256,
    &webpki::ECDSA_P256_SHA384,
    &webpki::ECDSA_P384_SHA256,
    &webpki::ECDSA_P384_SHA384,
    &webpki::RSA_PSS_2048_8192_SHA256_LEGACY_KEY,
    &webpki::RSA_PSS_2048_8192_SHA384_LEGACY_KEY,
    &webpki::RSA_PSS_2048_8192_SHA512_LEGACY_KEY,
    &webpki::RSA_PKCS1_2048_8192_SHA256,
    &webpki::RSA_PKCS1_2048_8192_SHA384,
    &webpki::RSA_PKCS1_2048_8192_SHA512,
    &webpki::RSA_PKCS1_3072_8192_SHA384,
];

/// AttestationReport can be endorsed by either the Intel Attestation Service
/// using EPID or Data Center Attestation
/// Service (platform dependent) using ECDSA.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndorsedAttestationVerificationReport {
    /// Attestation report generated by the hardware
    pub avr: String,
    /// Singature of the report
    #[serde(with = "serde_base64")]
    pub signature: Vec<u8>,
    /// Certificate matching the signing key of the signature
    #[serde(with = "serde_base64")]
    pub signing_cert: Vec<u8>,
}

impl EndorsedAttestationVerificationReport {
    pub fn get_avr(&self) -> Result<AttestationVerificationReport, Error> {
        Ok(serde_json::from_slice(self.avr.as_bytes()).map_err(Error::SerdeJSONError)?)
    }
}

// AttestationVerificationReport represents Intel's Attestation Verification Report
// https://api.trustedservices.intel.com/documents/sgx-attestation-api-spec.pdf
#[derive(Default, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct AttestationVerificationReport {
    pub id: String,
    pub timestamp: String,
    pub version: i64,
    #[serde(alias = "isvEnclaveQuoteStatus")]
    pub isv_enclave_quote_status: String,
    #[serde(alias = "isvEnclaveQuoteBody")]
    pub isv_enclave_quote_body: String,
    #[serde(alias = "revocationReason")]
    pub revocation_reason: Option<i64>,
    #[serde(alias = "pseManifestStatus")]
    pub pse_manifest_status: Option<i64>,
    #[serde(alias = "pseManifestHash")]
    pub pse_manifest_hash: Option<String>,
    #[serde(alias = "platformInfoBlob")]
    pub platform_info_blob: Option<String>,
    pub nonce: Option<String>,
    #[serde(alias = "epidPseudonym")]
    pub epid_pseudonym: Option<Vec<u8>>,
    #[serde(alias = "advisoryURL")]
    pub advisory_url: String,
    #[serde(alias = "advisoryIDs")]
    pub advisory_ids: Vec<String>,
}

impl TryFrom<&AttestationVerificationReport> for Vec<u8> {
    type Error = serde_json::Error;

    fn try_from(value: &AttestationVerificationReport) -> Result<Self, Self::Error> {
        serde_json::to_vec(value)
    }
}

impl TryFrom<&[u8]> for AttestationVerificationReport {
    type Error = serde_json::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        serde_json::from_slice(value)
    }
}

impl AttestationVerificationReport {
    pub fn parse_quote(&self) -> Result<Quote, Error> {
        if self.version != 4 {
            return Err(Error::UnexpectedAttestationReportVersionError(
                4,
                self.version,
            ));
        }

        let time_fixed = self.timestamp.clone() + "+0000";
        let dt = DateTime::parse_from_str(&time_fixed, "%Y-%m-%dT%H:%M:%S%.f%z").unwrap();

        let attestation_time =
            TmTime::from_unix_timestamp(dt.timestamp(), dt.timestamp_subsec_nanos())
                .map_err(lcp_types::TimeError::TendermintError)?
                .into();

        let quote = base64::decode(&self.isv_enclave_quote_body)?;
        let sgx_quote: sgx_quote_t = unsafe { ptr::read(quote.as_ptr() as *const _) };
        Ok(Quote {
            raw: sgx_quote,
            status: self.isv_enclave_quote_status.clone(),
            attestation_time,
        })
    }
}

pub fn verify_report(
    report: &EndorsedAttestationVerificationReport,
    current_time: Time,
) -> Result<(), Error> {
    let current_unix_timestamp = current_time.duration_since(TmTime::unix_epoch()).unwrap();
    // NOTE: Currently, webpki::Time's constructor only accepts seconds as unix timestamp.
    // Therefore, the current time are rounded up conservatively.
    let secs = if current_unix_timestamp.subsec_nanos() > 0 {
        current_unix_timestamp.as_secs()
    } else {
        current_unix_timestamp.as_secs() + 1
    };
    let now = webpki::Time::from_seconds_since_unix_epoch(secs);
    let root_ca_pem = pem::parse(IAS_REPORT_CA).expect("failed to parse pem bytes");
    let root_ca = root_ca_pem.contents;

    let mut root_store = rustls::RootCertStore::empty();
    root_store
        .add(&rustls::Certificate(root_ca.clone()))
        .map_err(Error::WebPKIError)?;

    let trust_anchors: Vec<webpki::TrustAnchor> = root_store
        .roots
        .iter()
        .map(|cert| cert.to_trust_anchor())
        .collect();

    let mut chain: Vec<&[u8]> = Vec::new();
    chain.push(&root_ca);

    let report_cert =
        webpki::EndEntityCert::from(&report.signing_cert).map_err(Error::WebPKIError)?;

    let _ = report_cert
        .verify_is_valid_tls_server_cert(
            SUPPORTED_SIG_ALGS,
            &webpki::TLSServerTrustAnchors(&trust_anchors),
            &chain,
            now,
        )
        .map_err(Error::WebPKIError)?;

    let _ = report_cert
        .verify_signature(
            &webpki::RSA_PKCS1_2048_8192_SHA256,
            report.avr.as_bytes(),
            &report.signature,
        )
        .map_err(Error::WebPKIError)?;

    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quote {
    pub raw: sgx_quote_t,
    pub status: String,
    pub attestation_time: Time,
}

impl Quote {
    pub fn get_enclave_key_address(&self) -> Result<Address, Error> {
        let data = self.raw.report_body.report_data.d;
        if data.len() < 20 {
            Err(Error::InvalidReportDataError(format!(
                "unexpected report data length: {}",
                data.len()
            )))
        } else {
            Ok(Address::from(&data[..20]))
        }
    }
}

mod serde_base64 {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        let base64 = base64::encode(v);
        String::serialize(&base64, s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let base64 = String::deserialize(d)?;
        base64::decode(base64.as_bytes()).map_err(|e| serde::de::Error::custom(e))
    }
}
