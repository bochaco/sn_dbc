// Copyright 2021 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// use blsttc::PublicKey;
// use blstrs::group::GroupEncoding;
use blstrs::{Scalar};
use blst_ringct::RevealedCommitment;
// use serde::{Deserialize, Serialize};
// use tiny_keccak::{Hasher, Sha3};
use blsttc::{DecryptionShare, IntoFr, SecretKey, SecretKeySet, SecretKeyShare, Ciphertext, PublicKey, PublicKeySet};
use std::convert::TryFrom;
use std::collections::BTreeMap;
use rand_core::OsRng;

use crate::{Amount, Error};

// note: Amount should move into blst_ringct crate.
// (or else blst_ringct::RevealedCommitment should be made generic over Amount type)

// pub type Amount = u64;
// pub type OwnerPublicKey = G1Affine;

const AMT_SIZE: usize = 8; // Amount size: 8 bytes (u64)
const BF_SIZE: usize = 32; // Blinding factor size: 32 bytes (Scalar)

pub struct AmountSecrets(RevealedCommitment);

impl AmountSecrets {

    pub fn amount(&self) -> Amount {
        self.0.value
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_bytes()
        // let mut v: Vec<u8> = Default::default();
        // v.extend(&self.amount.to_le_bytes());
        // v.extend(&self.blinding_factor.to_bytes());
        // v
    }

    /// build AmountSecrets from fixed size byte array.
    pub fn from_bytes(bytes: [u8; AMT_SIZE + BF_SIZE]) -> Self {
        let amount = Amount::from_le_bytes({
            let mut b = [0u8; AMT_SIZE];
            b.copy_from_slice(&bytes[0..AMT_SIZE]);
            b
        });
        let mut b = [0u8; BF_SIZE];
        let blinding_factor = Scalar::from_bytes_le({
            b.copy_from_slice(&bytes[AMT_SIZE..]);
            &b
        }).unwrap();

        Self(
            RevealedCommitment {
                value: amount,
                blinding: blinding_factor,
            }
        )
    }

    /// build AmountSecrets from byte array reference
    pub fn from_bytes_ref(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != AMT_SIZE + BF_SIZE {
            return Err(Error::AmountSecretsBytesInvalid);
        }
        let amount = Amount::from_le_bytes({
            let mut b = [0u8; AMT_SIZE];
            b.copy_from_slice(&bytes[0..AMT_SIZE]);
            b
        });
        let mut b = [0u8; BF_SIZE];
        let blinding_factor = Scalar::from_bytes_le({
            b.copy_from_slice(&bytes[AMT_SIZE..]);
            &b
        }).unwrap();
        Ok(Self (
            RevealedCommitment {
                value: amount,
                blinding: blinding_factor,
            }
        ))
    }

    /// generate a pedersen commitment
    // pub fn to_pedersen_commitment(&self) -> G1Projective {
    //     self.0.commit(&PedersenGens::default())
    // }

    /// encrypt secrets to public_key producing Ciphertext
    pub fn encrypt(&self, public_key: &PublicKey) -> Ciphertext {
        public_key.encrypt(&self.to_bytes())
    }

    // generate a random blinding factor
    // pub fn random_blinding_factor() -> Scalar {
    //     let mut csprng: OsRng = OsRng::default();
    //     Scalar::random(&mut csprng)
    // }
}

impl From<RevealedCommitment> for AmountSecrets {
    /// create AmountSecrets from an amount and a randomly generated blinding factor
    fn from(revealed_commitment: RevealedCommitment) -> Self {
        Self(revealed_commitment)
    }
}

impl From<Amount> for AmountSecrets {
    /// create AmountSecrets from an amount and a randomly generated blinding factor
    fn from(amount: Amount) -> Self {
        let mut rng = OsRng::default();
        Self(RevealedCommitment::from_value(amount, &mut rng))
    }
}

impl TryFrom<(&SecretKey, &Ciphertext)> for AmountSecrets {
    type Error = Error;

    /// Decrypt AmountSecrets ciphertext using a SecretKey
    fn try_from(params: (&SecretKey, &Ciphertext)) -> Result<Self, Error> {
        let (secret_key, ciphertext) = params;
        let bytes_vec = secret_key
            .decrypt(ciphertext)
            .ok_or(Error::DecryptionBySecretKeyFailed)?;
        Self::from_bytes_ref(&bytes_vec)
    }
}

impl TryFrom<(&SecretKeySet, &Ciphertext)> for AmountSecrets {
    type Error = Error;

    /// Decrypt AmountSecrets ciphertext using a SecretKeySet
    fn try_from(params: (&SecretKeySet, &Ciphertext)) -> Result<Self, Error> {
        let (secret_key_set, ciphertext) = params;
        Self::try_from((&secret_key_set.secret_key(), ciphertext))
    }
}

impl<I: IntoFr + Ord> TryFrom<(&PublicKeySet, &BTreeMap<I, SecretKeyShare>, &Ciphertext)>
    for AmountSecrets
{
    type Error = Error;

    /// Decrypt AmountSecrets ciphertext using threshold+1 SecretKeyShares
    fn try_from(
        params: (&PublicKeySet, &BTreeMap<I, SecretKeyShare>, &Ciphertext),
    ) -> Result<Self, Error> {
        let (public_key_set, secret_key_shares, ciphertext) = params;

        let mut decryption_shares: BTreeMap<I, DecryptionShare> = Default::default();
        for (idx, sec_share) in secret_key_shares.iter() {
            let share = sec_share.decrypt_share_no_verify(ciphertext);
            decryption_shares.insert(*idx, share);
        }
        Self::try_from((public_key_set, &decryption_shares, ciphertext))
    }
}

impl<I: IntoFr + Ord> TryFrom<(&PublicKeySet, &BTreeMap<I, DecryptionShare>, &Ciphertext)>
    for AmountSecrets
{
    type Error = Error;

    /// Decrypt AmountSecrets using threshold+1 DecryptionShares
    ///
    /// This fn should be used when keys (SecretKeyShare) are distributed across multiple parties.
    /// In which case each party will need to call SecretKeyShare::decrypt_share() or
    /// decrypt_share_no_verify() to generate a DecryptionShare and one party will need to
    /// obtain/aggregate all the shares together somehow.
    fn try_from(
        params: (&PublicKeySet, &BTreeMap<I, DecryptionShare>, &Ciphertext),
    ) -> Result<Self, Error> {
        let (public_key_set, decryption_shares, ciphertext) = params;
        let bytes_vec = public_key_set.decrypt(decryption_shares, ciphertext)?;
        Self::from_bytes_ref(&bytes_vec)
    }
}
