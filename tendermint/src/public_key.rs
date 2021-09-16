//! Public keys used in Tendermint networks

pub use ed25519_dalek::PublicKey as Ed25519;
#[cfg(feature = "secp256k1")]
pub use k256::ecdsa::VerifyingKey as Secp256k1;

mod pub_key_request;
mod pub_key_response;
pub use pub_key_request::PubKeyRequest;
pub use pub_key_response::PubKeyResponse;

use crate::{error::Error, signature::Signature};
use serde::{de, ser, Deserialize, Serialize};
use signature::Verifier as _;
use std::convert::TryFrom;
use std::{cmp::Ordering, fmt, ops::Deref, str::FromStr};
use subtle_encoding::{base64, bech32, hex};
use tendermint_proto::crypto::public_key::Sum;
use tendermint_proto::crypto::PublicKey as RawPublicKey;
use tendermint_proto::Protobuf;

// Note:On the golang side this is generic in the sense that it could everything that implements
// github.com/tendermint/tendermint/crypto.PubKey
// While this is meant to be used with different key-types, it currently only uses a PubKeyEd25519
// version.
// TODO: make this more generic

// Warning: the custom serialization implemented here does not use TryFrom<RawPublicKey>.
//          it should only be used to read/write the priva_validator_key.json.
//          All changes to the serialization should check both the JSON and protobuf conversions.
// Todo: Merge JSON serialization with #[serde(try_from = "RawPublicKey", into = "RawPublicKey)]
/// Public keys allowed in Tendermint protocols
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", content = "value")] // JSON custom serialization for priv_validator_key.json
pub enum PublicKey {
    /// Ed25519 keys
    #[serde(
        rename = "tendermint/PubKeyEd25519",
        serialize_with = "serialize_ed25519_base64",
        deserialize_with = "deserialize_ed25519_base64"
    )]
    Ed25519(Ed25519),

    /// Secp256k1 keys
    #[cfg(feature = "secp256k1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "secp256k1")))]
    #[serde(
        rename = "tendermint/PubKeySecp256k1",
        serialize_with = "serialize_secp256k1_base64",
        deserialize_with = "deserialize_secp256k1_base64"
    )]
    Secp256k1(Secp256k1),
}

impl Protobuf<RawPublicKey> for PublicKey {}

impl TryFrom<RawPublicKey> for PublicKey {
    type Error = Error;

    fn try_from(value: RawPublicKey) -> Result<Self, Self::Error> {
        let sum = &value
            .sum
            .ok_or_else(|| Error::invalid_key("empty sum".to_string()))?;
        if let Sum::Ed25519(b) = sum {
            return Self::from_raw_ed25519(b)
                .ok_or_else(|| Error::invalid_key("malformed ed25519 key".to_string()));
        }
        #[cfg(feature = "secp256k1")]
        if let Sum::Secp256k1(b) = sum {
            return Self::from_raw_secp256k1(b)
                .ok_or_else(|| Error::invalid_key("malformed key".to_string()));
        }
        Err(Error::invalid_key("not an ed25519 key".to_string()))
    }
}

impl From<PublicKey> for RawPublicKey {
    fn from(value: PublicKey) -> Self {
        match value {
            PublicKey::Ed25519(ref pk) => RawPublicKey {
                sum: Some(tendermint_proto::crypto::public_key::Sum::Ed25519(
                    pk.as_bytes().to_vec(),
                )),
            },
            #[cfg(feature = "secp256k1")]
            PublicKey::Secp256k1(ref pk) => RawPublicKey {
                sum: Some(tendermint_proto::crypto::public_key::Sum::Secp256k1(
                    pk.to_bytes().to_vec(),
                )),
            },
        }
    }
}

impl PublicKey {
    /// From raw secp256k1 public key bytes
    #[cfg(feature = "secp256k1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "secp256k1")))]
    pub fn from_raw_secp256k1(bytes: &[u8]) -> Option<PublicKey> {
        Secp256k1::from_sec1_bytes(bytes)
            .ok()
            .map(PublicKey::Secp256k1)
    }

    /// From raw Ed25519 public key bytes
    pub fn from_raw_ed25519(bytes: &[u8]) -> Option<PublicKey> {
        Ed25519::from_bytes(bytes).map(Into::into).ok()
    }

    /// Get Ed25519 public key
    pub fn ed25519(self) -> Option<Ed25519> {
        #[allow(unreachable_patterns)]
        match self {
            PublicKey::Ed25519(pk) => Some(pk),
            _ => None,
        }
    }

    /// Get Secp256k1 public key
    #[cfg(feature = "secp256k1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "secp256k1")))]
    pub fn secp256k1(self) -> Option<Secp256k1> {
        match self {
            PublicKey::Secp256k1(pk) => Some(pk),
            _ => None,
        }
    }

    /// Verify the given [`Signature`] using this public key
    pub fn verify(&self, msg: &[u8], signature: &Signature) -> Result<(), Error> {
        match self {
            PublicKey::Ed25519(pk) => {
                match ed25519_dalek::Signature::try_from(signature.as_bytes()) {
                    Ok(sig) => pk.verify(msg, &sig).map_err(|_| {
                        Error::signature_invalid(
                            "Ed25519 signature verification failed".to_string(),
                        )
                    }),
                    Err(e) => Err(Error::signature_invalid(format!(
                        "invalid Ed25519 signature: {}",
                        e
                    ))),
                }
            }
            #[cfg(feature = "secp256k1")]
            PublicKey::Secp256k1(pk) => {
                match k256::ecdsa::Signature::try_from(signature.as_bytes()) {
                    Ok(sig) => pk.verify(msg, &sig).map_err(|_| {
                        Error::signature_invalid(
                            "Secp256k1 signature verification failed".to_string(),
                        )
                    }),
                    Err(e) => Err(Error::signature_invalid(format!(
                        "invalid Secp256k1 signature: {}",
                        e
                    ))),
                }
            }
        }
    }

    /// Serialize this key as a byte vector.
    pub fn to_bytes(self) -> Vec<u8> {
        match self {
            PublicKey::Ed25519(pk) => pk.as_bytes().to_vec(),
            #[cfg(feature = "secp256k1")]
            PublicKey::Secp256k1(pk) => pk.to_bytes().to_vec(),
        }
    }

    /// Serialize this key as Bech32 with the given human readable prefix
    pub fn to_bech32(self, hrp: &str) -> String {
        let backward_compatible_amino_prefixed_pubkey = match self {
            PublicKey::Ed25519(ref pk) => {
                let mut key_bytes = vec![0x16, 0x24, 0xDE, 0x64, 0x20];
                key_bytes.extend(pk.as_bytes());
                key_bytes
            }
            #[cfg(feature = "secp256k1")]
            PublicKey::Secp256k1(ref pk) => {
                let mut key_bytes = vec![0xEB, 0x5A, 0xE9, 0x87, 0x21];
                key_bytes.extend(pk.to_bytes());
                key_bytes
            }
        };
        bech32::encode(hrp, backward_compatible_amino_prefixed_pubkey)
    }

    /// Serialize this key as hexadecimal
    pub fn to_hex(self) -> String {
        String::from_utf8(hex::encode_upper(self.to_bytes())).unwrap()
    }
}

impl From<Ed25519> for PublicKey {
    fn from(pk: Ed25519) -> PublicKey {
        PublicKey::Ed25519(pk)
    }
}

#[cfg(feature = "secp256k1")]
impl From<Secp256k1> for PublicKey {
    fn from(pk: Secp256k1) -> PublicKey {
        PublicKey::Secp256k1(pk)
    }
}

impl PartialOrd for PublicKey {
    fn partial_cmp(&self, other: &PublicKey) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PublicKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match self {
            PublicKey::Ed25519(a) => match other {
                PublicKey::Ed25519(b) => a.as_bytes().cmp(b.as_bytes()),
                #[cfg(feature = "secp256k1")]
                PublicKey::Secp256k1(_) => Ordering::Less,
            },
            #[cfg(feature = "secp256k1")]
            PublicKey::Secp256k1(a) => match other {
                PublicKey::Ed25519(_) => Ordering::Greater,
                #[cfg(feature = "secp256k1")]
                PublicKey::Secp256k1(b) => a.cmp(b),
            },
        }
    }
}

/// Public key roles used in Tendermint networks
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum TendermintKey {
    /// User signing keys used for interacting with accounts in the state machine
    AccountKey(PublicKey),

    /// Validator signing keys used for authenticating consensus protocol messages
    ConsensusKey(PublicKey),
}

impl TendermintKey {
    /// Create a new account key from a [`PublicKey`]
    pub fn new_account_key(public_key: PublicKey) -> Result<TendermintKey, Error> {
        match public_key {
            PublicKey::Ed25519(_) => Ok(TendermintKey::AccountKey(public_key)),
            #[cfg(feature = "secp256k1")]
            PublicKey::Secp256k1(_) => Ok(TendermintKey::AccountKey(public_key)),
        }
    }

    /// Create a new consensus key from a [`PublicKey`]
    pub fn new_consensus_key(public_key: PublicKey) -> Result<TendermintKey, Error> {
        #[allow(unreachable_patterns)]
        match public_key {
            PublicKey::Ed25519(_) => Ok(TendermintKey::AccountKey(public_key)),
            _ => Err(Error::invalid_key(
                "only ed25519 consensus keys are supported".to_string(),
            )),
        }
    }

    /// Get the [`PublicKey`] value for this [`TendermintKey`]
    pub fn public_key(&self) -> &PublicKey {
        match self {
            TendermintKey::AccountKey(key) => key,
            TendermintKey::ConsensusKey(key) => key,
        }
    }
}

// TODO(tarcieri): deprecate/remove this in favor of `TendermintKey::public_key`
impl Deref for TendermintKey {
    type Target = PublicKey;

    fn deref(&self) -> &PublicKey {
        self.public_key()
    }
}

/// Public key algorithms
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Algorithm {
    /// ed25519
    Ed25519,

    /// secp256k1
    Secp256k1,
}

impl Algorithm {
    /// Get the string label for this algorithm
    pub fn as_str(&self) -> &str {
        match self {
            Algorithm::Ed25519 => "ed25519",
            Algorithm::Secp256k1 => "secp256k1",
        }
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for Algorithm {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        match s {
            "ed25519" => Ok(Algorithm::Ed25519),
            "secp256k1" => Ok(Algorithm::Secp256k1),
            _ => Err(Error::parse(format!("invalid algorithm: {}", s))),
        }
    }
}

impl Serialize for Algorithm {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_str().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Algorithm {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use de::Error;
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(D::Error::custom)
    }
}

/// Serialize the bytes of an Ed25519 public key as Base64. Used for serializing JSON
fn serialize_ed25519_base64<S>(pk: &Ed25519, serializer: S) -> Result<S::Ok, S::Error>
where
    S: ser::Serializer,
{
    String::from_utf8(base64::encode(pk.as_bytes()))
        .unwrap()
        .serialize(serializer)
}

/// Serialize the bytes of a secp256k1 ECDSA public key as Base64. Used for serializing JSON
#[cfg(feature = "secp256k1")]
fn serialize_secp256k1_base64<S>(pk: &Secp256k1, serializer: S) -> Result<S::Ok, S::Error>
where
    S: ser::Serializer,
{
    String::from_utf8(base64::encode(pk.to_bytes()))
        .unwrap()
        .serialize(serializer)
}

fn deserialize_ed25519_base64<'de, D>(deserializer: D) -> Result<Ed25519, D::Error>
where
    D: de::Deserializer<'de>,
{
    use de::Error;
    let encoded = String::deserialize(deserializer)?;
    let bytes = base64::decode(&encoded).map_err(D::Error::custom)?;
    Ed25519::from_bytes(&bytes).map_err(D::Error::custom)
}

#[cfg(feature = "secp256k1")]
fn deserialize_secp256k1_base64<'de, D>(deserializer: D) -> Result<Secp256k1, D::Error>
where
    D: de::Deserializer<'de>,
{
    use de::Error;
    let encoded = String::deserialize(deserializer)?;
    let bytes = base64::decode(&encoded).map_err(D::Error::custom)?;
    Secp256k1::from_sec1_bytes(&bytes).map_err(|_| D::Error::custom("invalid secp256k1 key"))
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use super::{PublicKey, Signature, TendermintKey};
    use crate::public_key::PubKeyResponse;
    use subtle_encoding::hex;
    use tendermint_proto::Protobuf;

    const EXAMPLE_CONSENSUS_KEY: &str =
        "4A25C6640A1F72B9C975338294EF51B6D1C33158BB6ECBA69FBC3FB5A33C9DCE";

    #[test]
    fn test_consensus_serialization() {
        let example_key = TendermintKey::ConsensusKey(
            PublicKey::from_raw_ed25519(&hex::decode_upper(EXAMPLE_CONSENSUS_KEY).unwrap())
                .unwrap(),
        );
        /* Key created from:
        import (
            "encoding/hex"
            "fmt"
            "github.com/cosmos/cosmos-sdk/crypto/keys/ed25519"
            "github.com/cosmos/cosmos-sdk/types"
        )

        func bech32conspub() {
            pubBz, _ := hex.DecodeString("4A25C6640A1F72B9C975338294EF51B6D1C33158BB6ECBA69FBC3FB5A33C9DCE")
            pub := &ed25519.PubKey{Key: pubBz}
            mustBech32ConsPub := types.MustBech32ifyPubKey(types.Bech32PubKeyTypeConsPub, pub)
            fmt.Println(mustBech32ConsPub)
        }
         */
        assert_eq!(
            example_key.to_bech32("cosmosvalconspub"),
            "cosmosvalconspub1zcjduepqfgjuveq2raetnjt4xwpffm63kmguxv2chdhvhf5lhslmtgeunh8qmf7exk"
        );
    }

    #[test]
    #[cfg(feature = "secp256k1")]
    fn test_account_serialization() {
        const EXAMPLE_ACCOUNT_KEY: &str =
            "02A1633CAFCC01EBFB6D78E39F687A1F0995C62FC95F51EAD10A02EE0BE551B5DC";
        let example_key = TendermintKey::AccountKey(
            PublicKey::from_raw_secp256k1(&hex::decode_upper(EXAMPLE_ACCOUNT_KEY).unwrap())
                .unwrap(),
        );
        assert_eq!(
            example_key.to_bech32("cosmospub"),
            "cosmospub1addwnpepq2skx090esq7h7md0r3e76r6ruyet330e904r6k3pgpwuzl92x6actrt4uq"
        );
    }

    #[test]
    fn json_parsing() {
        let json_string = "{\"type\":\"tendermint/PubKeyEd25519\",\"value\":\"RblzMO4is5L1hZz6wo4kPbptzOyue6LTk4+lPhD1FRk=\"}";
        let pubkey: PublicKey = serde_json::from_str(json_string).unwrap();

        assert_eq!(
            pubkey.ed25519().unwrap().as_ref(),
            [
                69, 185, 115, 48, 238, 34, 179, 146, 245, 133, 156, 250, 194, 142, 36, 61, 186,
                109, 204, 236, 174, 123, 162, 211, 147, 143, 165, 62, 16, 245, 21, 25
            ]
        );

        let reserialized_json = serde_json::to_string(&pubkey).unwrap();
        assert_eq!(reserialized_json.as_str(), json_string);
    }

    #[test]
    fn test_ed25519_pubkey_msg() {
        // test-vector generated from Go
        /*
           import (
               "fmt"
               "github.com/tendermint/tendermint/proto/tendermint/crypto"
               "github.com/tendermint/tendermint/proto/tendermint/privval"
           )

            func ed25519_key() {
                pkr := &privval.PubKeyResponse{
                    PubKey: &crypto.PublicKey{
                        Sum: &crypto.PublicKey_Ed25519{Ed25519: []byte{
                            215, 90, 152, 1, 130, 177, 10, 183, 213, 75, 254, 211, 201, 100, 7, 58,
                            14, 225, 114, 243, 218, 166, 35, 37, 175, 2, 26, 104, 247, 7, 81, 26,
                        },
                        },
                    },
                    Error: nil,
                }
                pbpk, _ := pkr.Marshal()
                fmt.Printf("%#v\n", pbpk)

            }
        */
        let encoded = vec![
            0xa, 0x22, 0xa, 0x20, 0xd7, 0x5a, 0x98, 0x1, 0x82, 0xb1, 0xa, 0xb7, 0xd5, 0x4b, 0xfe,
            0xd3, 0xc9, 0x64, 0x7, 0x3a, 0xe, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x2,
            0x1a, 0x68, 0xf7, 0x7, 0x51, 0x1a,
        ];

        let msg = PubKeyResponse {
            pub_key: Some(
                PublicKey::from_raw_ed25519(&[
                    215, 90, 152, 1, 130, 177, 10, 183, 213, 75, 254, 211, 201, 100, 7, 58, 14,
                    225, 114, 243, 218, 166, 35, 37, 175, 2, 26, 104, 247, 7, 81, 26,
                ])
                .unwrap(),
            ),
            error: None,
        };
        let got = msg.encode_vec().unwrap();

        assert_eq!(got, encoded);
        assert_eq!(PubKeyResponse::decode_vec(&encoded).unwrap(), msg);
    }

    // From https://datatracker.ietf.org/doc/html/rfc8032#section-7.1
    // Each test vector consists of: [public_key, message, signature].
    const ED25519_TEST_VECTORS: &[&[&[u8]]] = &[
        // Test 1
        &[
            &[
                0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
                0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
                0xf7, 0x07, 0x51, 0x1a,
            ],
            &[],
            &[
                0xe5, 0x56, 0x43, 0x00, 0xc3, 0x60, 0xac, 0x72, 0x90, 0x86, 0xe2, 0xcc, 0x80, 0x6e,
                0x82, 0x8a, 0x84, 0x87, 0x7f, 0x1e, 0xb8, 0xe5, 0xd9, 0x74, 0xd8, 0x73, 0xe0, 0x65,
                0x22, 0x49, 0x01, 0x55, 0x5f, 0xb8, 0x82, 0x15, 0x90, 0xa3, 0x3b, 0xac, 0xc6, 0x1e,
                0x39, 0x70, 0x1c, 0xf9, 0xb4, 0x6b, 0xd2, 0x5b, 0xf5, 0xf0, 0x59, 0x5b, 0xbe, 0x24,
                0x65, 0x51, 0x41, 0x43, 0x8e, 0x7a, 0x10, 0x0b,
            ],
        ],
        // Test 2
        &[
            &[
                0x3d, 0x40, 0x17, 0xc3, 0xe8, 0x43, 0x89, 0x5a, 0x92, 0xb7, 0x0a, 0xa7, 0x4d, 0x1b,
                0x7e, 0xbc, 0x9c, 0x98, 0x2c, 0xcf, 0x2e, 0xc4, 0x96, 0x8c, 0xc0, 0xcd, 0x55, 0xf1,
                0x2a, 0xf4, 0x66, 0x0c,
            ],
            &[0x72],
            &[
                0x92, 0xa0, 0x09, 0xa9, 0xf0, 0xd4, 0xca, 0xb8, 0x72, 0x0e, 0x82, 0x0b, 0x5f, 0x64,
                0x25, 0x40, 0xa2, 0xb2, 0x7b, 0x54, 0x16, 0x50, 0x3f, 0x8f, 0xb3, 0x76, 0x22, 0x23,
                0xeb, 0xdb, 0x69, 0xda, 0x08, 0x5a, 0xc1, 0xe4, 0x3e, 0x15, 0x99, 0x6e, 0x45, 0x8f,
                0x36, 0x13, 0xd0, 0xf1, 0x1d, 0x8c, 0x38, 0x7b, 0x2e, 0xae, 0xb4, 0x30, 0x2a, 0xee,
                0xb0, 0x0d, 0x29, 0x16, 0x12, 0xbb, 0x0c, 0x00,
            ],
        ],
        // Test 3
        &[
            &[
                0xfc, 0x51, 0xcd, 0x8e, 0x62, 0x18, 0xa1, 0xa3, 0x8d, 0xa4, 0x7e, 0xd0, 0x02, 0x30,
                0xf0, 0x58, 0x08, 0x16, 0xed, 0x13, 0xba, 0x33, 0x03, 0xac, 0x5d, 0xeb, 0x91, 0x15,
                0x48, 0x90, 0x80, 0x25,
            ],
            &[0xaf, 0x82],
            &[
                0x62, 0x91, 0xd6, 0x57, 0xde, 0xec, 0x24, 0x02, 0x48, 0x27, 0xe6, 0x9c, 0x3a, 0xbe,
                0x01, 0xa3, 0x0c, 0xe5, 0x48, 0xa2, 0x84, 0x74, 0x3a, 0x44, 0x5e, 0x36, 0x80, 0xd7,
                0xdb, 0x5a, 0xc3, 0xac, 0x18, 0xff, 0x9b, 0x53, 0x8d, 0x16, 0xf2, 0x90, 0xae, 0x67,
                0xf7, 0x60, 0x98, 0x4d, 0xc6, 0x59, 0x4a, 0x7c, 0x15, 0xe9, 0x71, 0x6e, 0xd2, 0x8d,
                0xc0, 0x27, 0xbe, 0xce, 0xea, 0x1e, 0xc4, 0x0a,
            ],
        ],
        // Test 1024
        &[
            &[
                0x27, 0x81, 0x17, 0xfc, 0x14, 0x4c, 0x72, 0x34, 0x0f, 0x67, 0xd0, 0xf2, 0x31, 0x6e,
                0x83, 0x86, 0xce, 0xff, 0xbf, 0x2b, 0x24, 0x28, 0xc9, 0xc5, 0x1f, 0xef, 0x7c, 0x59,
                0x7f, 0x1d, 0x42, 0x6e,
            ],
            &[
                0x08, 0xb8, 0xb2, 0xb7, 0x33, 0x42, 0x42, 0x43, 0x76, 0x0f, 0xe4, 0x26, 0xa4, 0xb5,
                0x49, 0x08, 0x63, 0x21, 0x10, 0xa6, 0x6c, 0x2f, 0x65, 0x91, 0xea, 0xbd, 0x33, 0x45,
                0xe3, 0xe4, 0xeb, 0x98, 0xfa, 0x6e, 0x26, 0x4b, 0xf0, 0x9e, 0xfe, 0x12, 0xee, 0x50,
                0xf8, 0xf5, 0x4e, 0x9f, 0x77, 0xb1, 0xe3, 0x55, 0xf6, 0xc5, 0x05, 0x44, 0xe2, 0x3f,
                0xb1, 0x43, 0x3d, 0xdf, 0x73, 0xbe, 0x84, 0xd8, 0x79, 0xde, 0x7c, 0x00, 0x46, 0xdc,
                0x49, 0x96, 0xd9, 0xe7, 0x73, 0xf4, 0xbc, 0x9e, 0xfe, 0x57, 0x38, 0x82, 0x9a, 0xdb,
                0x26, 0xc8, 0x1b, 0x37, 0xc9, 0x3a, 0x1b, 0x27, 0x0b, 0x20, 0x32, 0x9d, 0x65, 0x86,
                0x75, 0xfc, 0x6e, 0xa5, 0x34, 0xe0, 0x81, 0x0a, 0x44, 0x32, 0x82, 0x6b, 0xf5, 0x8c,
                0x94, 0x1e, 0xfb, 0x65, 0xd5, 0x7a, 0x33, 0x8b, 0xbd, 0x2e, 0x26, 0x64, 0x0f, 0x89,
                0xff, 0xbc, 0x1a, 0x85, 0x8e, 0xfc, 0xb8, 0x55, 0x0e, 0xe3, 0xa5, 0xe1, 0x99, 0x8b,
                0xd1, 0x77, 0xe9, 0x3a, 0x73, 0x63, 0xc3, 0x44, 0xfe, 0x6b, 0x19, 0x9e, 0xe5, 0xd0,
                0x2e, 0x82, 0xd5, 0x22, 0xc4, 0xfe, 0xba, 0x15, 0x45, 0x2f, 0x80, 0x28, 0x8a, 0x82,
                0x1a, 0x57, 0x91, 0x16, 0xec, 0x6d, 0xad, 0x2b, 0x3b, 0x31, 0x0d, 0xa9, 0x03, 0x40,
                0x1a, 0xa6, 0x21, 0x00, 0xab, 0x5d, 0x1a, 0x36, 0x55, 0x3e, 0x06, 0x20, 0x3b, 0x33,
                0x89, 0x0c, 0xc9, 0xb8, 0x32, 0xf7, 0x9e, 0xf8, 0x05, 0x60, 0xcc, 0xb9, 0xa3, 0x9c,
                0xe7, 0x67, 0x96, 0x7e, 0xd6, 0x28, 0xc6, 0xad, 0x57, 0x3c, 0xb1, 0x16, 0xdb, 0xef,
                0xef, 0xd7, 0x54, 0x99, 0xda, 0x96, 0xbd, 0x68, 0xa8, 0xa9, 0x7b, 0x92, 0x8a, 0x8b,
                0xbc, 0x10, 0x3b, 0x66, 0x21, 0xfc, 0xde, 0x2b, 0xec, 0xa1, 0x23, 0x1d, 0x20, 0x6b,
                0xe6, 0xcd, 0x9e, 0xc7, 0xaf, 0xf6, 0xf6, 0xc9, 0x4f, 0xcd, 0x72, 0x04, 0xed, 0x34,
                0x55, 0xc6, 0x8c, 0x83, 0xf4, 0xa4, 0x1d, 0xa4, 0xaf, 0x2b, 0x74, 0xef, 0x5c, 0x53,
                0xf1, 0xd8, 0xac, 0x70, 0xbd, 0xcb, 0x7e, 0xd1, 0x85, 0xce, 0x81, 0xbd, 0x84, 0x35,
                0x9d, 0x44, 0x25, 0x4d, 0x95, 0x62, 0x9e, 0x98, 0x55, 0xa9, 0x4a, 0x7c, 0x19, 0x58,
                0xd1, 0xf8, 0xad, 0xa5, 0xd0, 0x53, 0x2e, 0xd8, 0xa5, 0xaa, 0x3f, 0xb2, 0xd1, 0x7b,
                0xa7, 0x0e, 0xb6, 0x24, 0x8e, 0x59, 0x4e, 0x1a, 0x22, 0x97, 0xac, 0xbb, 0xb3, 0x9d,
                0x50, 0x2f, 0x1a, 0x8c, 0x6e, 0xb6, 0xf1, 0xce, 0x22, 0xb3, 0xde, 0x1a, 0x1f, 0x40,
                0xcc, 0x24, 0x55, 0x41, 0x19, 0xa8, 0x31, 0xa9, 0xaa, 0xd6, 0x07, 0x9c, 0xad, 0x88,
                0x42, 0x5d, 0xe6, 0xbd, 0xe1, 0xa9, 0x18, 0x7e, 0xbb, 0x60, 0x92, 0xcf, 0x67, 0xbf,
                0x2b, 0x13, 0xfd, 0x65, 0xf2, 0x70, 0x88, 0xd7, 0x8b, 0x7e, 0x88, 0x3c, 0x87, 0x59,
                0xd2, 0xc4, 0xf5, 0xc6, 0x5a, 0xdb, 0x75, 0x53, 0x87, 0x8a, 0xd5, 0x75, 0xf9, 0xfa,
                0xd8, 0x78, 0xe8, 0x0a, 0x0c, 0x9b, 0xa6, 0x3b, 0xcb, 0xcc, 0x27, 0x32, 0xe6, 0x94,
                0x85, 0xbb, 0xc9, 0xc9, 0x0b, 0xfb, 0xd6, 0x24, 0x81, 0xd9, 0x08, 0x9b, 0xec, 0xcf,
                0x80, 0xcf, 0xe2, 0xdf, 0x16, 0xa2, 0xcf, 0x65, 0xbd, 0x92, 0xdd, 0x59, 0x7b, 0x07,
                0x07, 0xe0, 0x91, 0x7a, 0xf4, 0x8b, 0xbb, 0x75, 0xfe, 0xd4, 0x13, 0xd2, 0x38, 0xf5,
                0x55, 0x5a, 0x7a, 0x56, 0x9d, 0x80, 0xc3, 0x41, 0x4a, 0x8d, 0x08, 0x59, 0xdc, 0x65,
                0xa4, 0x61, 0x28, 0xba, 0xb2, 0x7a, 0xf8, 0x7a, 0x71, 0x31, 0x4f, 0x31, 0x8c, 0x78,
                0x2b, 0x23, 0xeb, 0xfe, 0x80, 0x8b, 0x82, 0xb0, 0xce, 0x26, 0x40, 0x1d, 0x2e, 0x22,
                0xf0, 0x4d, 0x83, 0xd1, 0x25, 0x5d, 0xc5, 0x1a, 0xdd, 0xd3, 0xb7, 0x5a, 0x2b, 0x1a,
                0xe0, 0x78, 0x45, 0x04, 0xdf, 0x54, 0x3a, 0xf8, 0x96, 0x9b, 0xe3, 0xea, 0x70, 0x82,
                0xff, 0x7f, 0xc9, 0x88, 0x8c, 0x14, 0x4d, 0xa2, 0xaf, 0x58, 0x42, 0x9e, 0xc9, 0x60,
                0x31, 0xdb, 0xca, 0xd3, 0xda, 0xd9, 0xaf, 0x0d, 0xcb, 0xaa, 0xaf, 0x26, 0x8c, 0xb8,
                0xfc, 0xff, 0xea, 0xd9, 0x4f, 0x3c, 0x7c, 0xa4, 0x95, 0xe0, 0x56, 0xa9, 0xb4, 0x7a,
                0xcd, 0xb7, 0x51, 0xfb, 0x73, 0xe6, 0x66, 0xc6, 0xc6, 0x55, 0xad, 0xe8, 0x29, 0x72,
                0x97, 0xd0, 0x7a, 0xd1, 0xba, 0x5e, 0x43, 0xf1, 0xbc, 0xa3, 0x23, 0x01, 0x65, 0x13,
                0x39, 0xe2, 0x29, 0x04, 0xcc, 0x8c, 0x42, 0xf5, 0x8c, 0x30, 0xc0, 0x4a, 0xaf, 0xdb,
                0x03, 0x8d, 0xda, 0x08, 0x47, 0xdd, 0x98, 0x8d, 0xcd, 0xa6, 0xf3, 0xbf, 0xd1, 0x5c,
                0x4b, 0x4c, 0x45, 0x25, 0x00, 0x4a, 0xa0, 0x6e, 0xef, 0xf8, 0xca, 0x61, 0x78, 0x3a,
                0xac, 0xec, 0x57, 0xfb, 0x3d, 0x1f, 0x92, 0xb0, 0xfe, 0x2f, 0xd1, 0xa8, 0x5f, 0x67,
                0x24, 0x51, 0x7b, 0x65, 0xe6, 0x14, 0xad, 0x68, 0x08, 0xd6, 0xf6, 0xee, 0x34, 0xdf,
                0xf7, 0x31, 0x0f, 0xdc, 0x82, 0xae, 0xbf, 0xd9, 0x04, 0xb0, 0x1e, 0x1d, 0xc5, 0x4b,
                0x29, 0x27, 0x09, 0x4b, 0x2d, 0xb6, 0x8d, 0x6f, 0x90, 0x3b, 0x68, 0x40, 0x1a, 0xde,
                0xbf, 0x5a, 0x7e, 0x08, 0xd7, 0x8f, 0xf4, 0xef, 0x5d, 0x63, 0x65, 0x3a, 0x65, 0x04,
                0x0c, 0xf9, 0xbf, 0xd4, 0xac, 0xa7, 0x98, 0x4a, 0x74, 0xd3, 0x71, 0x45, 0x98, 0x67,
                0x80, 0xfc, 0x0b, 0x16, 0xac, 0x45, 0x16, 0x49, 0xde, 0x61, 0x88, 0xa7, 0xdb, 0xdf,
                0x19, 0x1f, 0x64, 0xb5, 0xfc, 0x5e, 0x2a, 0xb4, 0x7b, 0x57, 0xf7, 0xf7, 0x27, 0x6c,
                0xd4, 0x19, 0xc1, 0x7a, 0x3c, 0xa8, 0xe1, 0xb9, 0x39, 0xae, 0x49, 0xe4, 0x88, 0xac,
                0xba, 0x6b, 0x96, 0x56, 0x10, 0xb5, 0x48, 0x01, 0x09, 0xc8, 0xb1, 0x7b, 0x80, 0xe1,
                0xb7, 0xb7, 0x50, 0xdf, 0xc7, 0x59, 0x8d, 0x5d, 0x50, 0x11, 0xfd, 0x2d, 0xcc, 0x56,
                0x00, 0xa3, 0x2e, 0xf5, 0xb5, 0x2a, 0x1e, 0xcc, 0x82, 0x0e, 0x30, 0x8a, 0xa3, 0x42,
                0x72, 0x1a, 0xac, 0x09, 0x43, 0xbf, 0x66, 0x86, 0xb6, 0x4b, 0x25, 0x79, 0x37, 0x65,
                0x04, 0xcc, 0xc4, 0x93, 0xd9, 0x7e, 0x6a, 0xed, 0x3f, 0xb0, 0xf9, 0xcd, 0x71, 0xa4,
                0x3d, 0xd4, 0x97, 0xf0, 0x1f, 0x17, 0xc0, 0xe2, 0xcb, 0x37, 0x97, 0xaa, 0x2a, 0x2f,
                0x25, 0x66, 0x56, 0x16, 0x8e, 0x6c, 0x49, 0x6a, 0xfc, 0x5f, 0xb9, 0x32, 0x46, 0xf6,
                0xb1, 0x11, 0x63, 0x98, 0xa3, 0x46, 0xf1, 0xa6, 0x41, 0xf3, 0xb0, 0x41, 0xe9, 0x89,
                0xf7, 0x91, 0x4f, 0x90, 0xcc, 0x2c, 0x7f, 0xff, 0x35, 0x78, 0x76, 0xe5, 0x06, 0xb5,
                0x0d, 0x33, 0x4b, 0xa7, 0x7c, 0x22, 0x5b, 0xc3, 0x07, 0xba, 0x53, 0x71, 0x52, 0xf3,
                0xf1, 0x61, 0x0e, 0x4e, 0xaf, 0xe5, 0x95, 0xf6, 0xd9, 0xd9, 0x0d, 0x11, 0xfa, 0xa9,
                0x33, 0xa1, 0x5e, 0xf1, 0x36, 0x95, 0x46, 0x86, 0x8a, 0x7f, 0x3a, 0x45, 0xa9, 0x67,
                0x68, 0xd4, 0x0f, 0xd9, 0xd0, 0x34, 0x12, 0xc0, 0x91, 0xc6, 0x31, 0x5c, 0xf4, 0xfd,
                0xe7, 0xcb, 0x68, 0x60, 0x69, 0x37, 0x38, 0x0d, 0xb2, 0xea, 0xaa, 0x70, 0x7b, 0x4c,
                0x41, 0x85, 0xc3, 0x2e, 0xdd, 0xcd, 0xd3, 0x06, 0x70, 0x5e, 0x4d, 0xc1, 0xff, 0xc8,
                0x72, 0xee, 0xee, 0x47, 0x5a, 0x64, 0xdf, 0xac, 0x86, 0xab, 0xa4, 0x1c, 0x06, 0x18,
                0x98, 0x3f, 0x87, 0x41, 0xc5, 0xef, 0x68, 0xd3, 0xa1, 0x01, 0xe8, 0xa3, 0xb8, 0xca,
                0xc6, 0x0c, 0x90, 0x5c, 0x15, 0xfc, 0x91, 0x08, 0x40, 0xb9, 0x4c, 0x00, 0xa0, 0xb9,
                0xd0,
            ],
            &[
                0x0a, 0xab, 0x4c, 0x90, 0x05, 0x01, 0xb3, 0xe2, 0x4d, 0x7c, 0xdf, 0x46, 0x63, 0x32,
                0x6a, 0x3a, 0x87, 0xdf, 0x5e, 0x48, 0x43, 0xb2, 0xcb, 0xdb, 0x67, 0xcb, 0xf6, 0xe4,
                0x60, 0xfe, 0xc3, 0x50, 0xaa, 0x53, 0x71, 0xb1, 0x50, 0x8f, 0x9f, 0x45, 0x28, 0xec,
                0xea, 0x23, 0xc4, 0x36, 0xd9, 0x4b, 0x5e, 0x8f, 0xcd, 0x4f, 0x68, 0x1e, 0x30, 0xa6,
                0xac, 0x00, 0xa9, 0x70, 0x4a, 0x18, 0x8a, 0x03,
            ],
        ],
    ];

    #[test]
    fn ed25519_test_vectors() {
        for (i, v) in ED25519_TEST_VECTORS.iter().enumerate() {
            let public_key = v[0];
            let msg = v[1];
            let sig = v[2];

            dbg!(String::from_utf8(hex::encode(&public_key)).unwrap());
            dbg!(String::from_utf8(hex::encode(&msg)).unwrap());
            dbg!(String::from_utf8(hex::encode(&sig)).unwrap());

            #[allow(unused_must_use)]
            {
                use ed25519_consensus as edc;
                let sig = edc::Signature::try_from(sig).unwrap();
                let result = edc::VerificationKey::try_from(public_key)
                    .and_then(|vk| vk.verify(&sig, msg));
                dbg!(result);
            }

            let public_key = PublicKey::from_raw_ed25519(public_key).unwrap();
            match public_key {
                PublicKey::Ed25519(_) => {}
                #[cfg(feature = "secp256k1")]
                _ => panic!("expected public key to be Ed25519: {:?}", public_key),
            }
            let sig = Signature::try_from(sig).unwrap();
            public_key
                .verify(msg, &sig)
                .unwrap_or_else(|_| panic!("signature should be valid for test vector {}", i));
        }
    }

    // Arbitrary "valid" tests taken from
    // https://github.com/google/wycheproof/blob/2196000605e45d91097147c9c71f26b72af58003/testvectors/ecdsa_secp256k1_sha256_test.json
    //
    // Each test vector consists of: [public_key, message, signature].
    //
    // NB: It appears as though all signatures in this test suite are
    // DER-encoded.
    #[cfg(feature = "secp256k1")]
    const SECP256K1_TEST_VECTORS: &[&[&[u8]]] = &[
        // tcId 3
        &[
            &[
                0x04, 0xb8, 0x38, 0xff, 0x44, 0xe5, 0xbc, 0x17, 0x7b, 0xf2, 0x11, 0x89, 0xd0, 0x76,
                0x60, 0x82, 0xfc, 0x9d, 0x84, 0x32, 0x26, 0x88, 0x7f, 0xc9, 0x76, 0x03, 0x71, 0x10,
                0x0b, 0x7e, 0xe2, 0x0a, 0x6f, 0xf0, 0xc9, 0xd7, 0x5b, 0xfb, 0xa7, 0xb3, 0x1a, 0x6b,
                0xca, 0x19, 0x74, 0x49, 0x6e, 0xeb, 0x56, 0xde, 0x35, 0x70, 0x71, 0x95, 0x5d, 0x83,
                0xc4, 0xb1, 0xba, 0xda, 0xa0, 0xb2, 0x18, 0x32, 0xe9,
            ],
            &[0x31, 0x32, 0x33, 0x34, 0x30, 0x30],
            &[
                0x30, 0x45, 0x02, 0x21, 0x00, 0x81, 0x3e, 0xf7, 0x9c, 0xce, 0xfa, 0x9a, 0x56, 0xf7,
                0xba, 0x80, 0x5f, 0x0e, 0x47, 0x85, 0x84, 0xfe, 0x5f, 0x0d, 0xd5, 0xf5, 0x67, 0xbc,
                0x09, 0xb5, 0x12, 0x3c, 0xcb, 0xc9, 0x83, 0x23, 0x65, 0x02, 0x20, 0x6f, 0xf1, 0x8a,
                0x52, 0xdc, 0xc0, 0x33, 0x6f, 0x7a, 0xf6, 0x24, 0x00, 0xa6, 0xdd, 0x9b, 0x81, 0x07,
                0x32, 0xba, 0xf1, 0xff, 0x75, 0x80, 0x00, 0xd6, 0xf6, 0x13, 0xa5, 0x56, 0xeb, 0x31,
                0xba,
            ],
        ],
        // tcId 230
        &[
            &[
                0x04, 0xb8, 0x38, 0xff, 0x44, 0xe5, 0xbc, 0x17, 0x7b, 0xf2, 0x11, 0x89, 0xd0, 0x76,
                0x60, 0x82, 0xfc, 0x9d, 0x84, 0x32, 0x26, 0x88, 0x7f, 0xc9, 0x76, 0x03, 0x71, 0x10,
                0x0b, 0x7e, 0xe2, 0x0a, 0x6f, 0xf0, 0xc9, 0xd7, 0x5b, 0xfb, 0xa7, 0xb3, 0x1a, 0x6b,
                0xca, 0x19, 0x74, 0x49, 0x6e, 0xeb, 0x56, 0xde, 0x35, 0x70, 0x71, 0x95, 0x5d, 0x83,
                0xc4, 0xb1, 0xba, 0xda, 0xa0, 0xb2, 0x18, 0x32, 0xe9,
            ],
            &[0x32, 0x35, 0x35, 0x38, 0x35],
            &[
                0x30, 0x45, 0x02, 0x21, 0x00, 0xdd, 0x1b, 0x7d, 0x09, 0xa7, 0xbd, 0x82, 0x18, 0x96,
                0x10, 0x34, 0xa3, 0x9a, 0x87, 0xfe, 0xcf, 0x53, 0x14, 0xf0, 0x0c, 0x4d, 0x25, 0xeb,
                0x58, 0xa0, 0x7a, 0xc8, 0x5e, 0x85, 0xea, 0xb5, 0x16, 0x02, 0x20, 0x35, 0x13, 0x8c,
                0x40, 0x1e, 0xf8, 0xd3, 0x49, 0x3d, 0x65, 0xc9, 0x00, 0x2f, 0xe6, 0x2b, 0x43, 0xae,
                0xe5, 0x68, 0x73, 0x1b, 0x74, 0x45, 0x48, 0x35, 0x89, 0x96, 0xd9, 0xcc, 0x42, 0x7e,
                0x06,
            ],
        ],
        // tcId 231
        &[
            &[
                0x04, 0xb8, 0x38, 0xff, 0x44, 0xe5, 0xbc, 0x17, 0x7b, 0xf2, 0x11, 0x89, 0xd0, 0x76,
                0x60, 0x82, 0xfc, 0x9d, 0x84, 0x32, 0x26, 0x88, 0x7f, 0xc9, 0x76, 0x03, 0x71, 0x10,
                0x0b, 0x7e, 0xe2, 0x0a, 0x6f, 0xf0, 0xc9, 0xd7, 0x5b, 0xfb, 0xa7, 0xb3, 0x1a, 0x6b,
                0xca, 0x19, 0x74, 0x49, 0x6e, 0xeb, 0x56, 0xde, 0x35, 0x70, 0x71, 0x95, 0x5d, 0x83,
                0xc4, 0xb1, 0xba, 0xda, 0xa0, 0xb2, 0x18, 0x32, 0xe9,
            ],
            &[0x34, 0x32, 0x36, 0x34, 0x37, 0x39, 0x37, 0x32, 0x34],
            &[
                0x30, 0x45, 0x02, 0x21, 0x00, 0x95, 0xc2, 0x92, 0x67, 0xd9, 0x72, 0xa0, 0x43, 0xd9,
                0x55, 0x22, 0x45, 0x46, 0x22, 0x2b, 0xba, 0x34, 0x3f, 0xc1, 0xd4, 0xdb, 0x0f, 0xec,
                0x26, 0x2a, 0x33, 0xac, 0x61, 0x30, 0x56, 0x96, 0xae, 0x02, 0x20, 0x6e, 0xdf, 0xe9,
                0x67, 0x13, 0xae, 0xd5, 0x6f, 0x8a, 0x28, 0xa6, 0x65, 0x3f, 0x57, 0xe0, 0xb8, 0x29,
                0x71, 0x2e, 0x5e, 0xdd, 0xc6, 0x7f, 0x34, 0x68, 0x2b, 0x24, 0xf0, 0x67, 0x6b, 0x26,
                0x40,
            ],
        ],
    ];

    #[cfg(feature = "secp256k1")]
    #[test]
    fn secp256k1_test_vectors() {
        for (i, v) in SECP256K1_TEST_VECTORS.iter().enumerate() {
            let public_key = v[0];
            let msg = v[1];
            let sig = v[2];

            let public_key = PublicKey::from_raw_secp256k1(public_key).unwrap();
            match public_key {
                PublicKey::Secp256k1(_) => {}
                _ => panic!("expected public key to be secp256k1: {:?}", public_key),
            }
            let der_sig = k256::ecdsa::Signature::from_der(sig).unwrap();
            let sig = der_sig.as_ref();
            let sig = Signature::try_from(sig).unwrap();
            public_key
                .verify(msg, &sig)
                .unwrap_or_else(|_| panic!("signature should be valid for test vector {}", i));
        }
    }
}
