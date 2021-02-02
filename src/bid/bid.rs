// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//
// Copyright (c) DUSK NETWORK. All rights reserved.

//! Bid data structure

use crate::errors::BlindBidError;
#[cfg(feature = "canon")]
use canonical::Canon;
#[cfg(feature = "canon")]
use canonical_derive::Canon;
use dusk_bls12_381::BlsScalar;
use dusk_bytes::{DeserializableSlice, Serializable};
use dusk_jubjub::{
    JubJubAffine, JubJubScalar, GENERATOR_EXTENDED, GENERATOR_NUMS_EXTENDED,
};
use dusk_pki::{Ownable, StealthAddress};
use poseidon252::cipher::PoseidonCipher;
use poseidon252::sponge;
use rand_core::{CryptoRng, RngCore};

#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "canon", derive(Canon))]
pub struct Bid {
    // b_enc (encrypted value and blinder)
    pub encrypted_data: PoseidonCipher,
    // Nonce used by the cypher
    pub nonce: BlsScalar,
    // Stealth address of the bidder
    pub stealth_address: StealthAddress,
    // m
    pub hashed_secret: BlsScalar,
    // c (Pedersen Commitment)
    pub c: JubJubAffine,
    // Elegibility timestamp
    pub eligibility: u64,
    // Expiration timestamp
    pub expiration: u64,
    // Position of the Bid in the BidTree
    pub pos: u64,
}

impl Ownable for Bid {
    fn stealth_address(&self) -> &StealthAddress {
        &self.stealth_address
    }
}

impl PartialEq for Bid {
    fn eq(&self, other: &Self) -> bool {
        self.hash().eq(&other.hash())
    }
}

// This needs to be between braces since const fn calls passed as const_generics
// params aren't perfectly supported yet.
impl
    Serializable<
        {
            PoseidonCipher::SIZE
                + StealthAddress::SIZE
                + 2 * BlsScalar::SIZE
                + JubJubAffine::SIZE
                + 8 * 3
        },
    > for Bid
{
    type Error = dusk_bytes::Error;

    fn from_bytes(buf: &[u8; Self::SIZE]) -> Result<Bid, Self::Error> {
        let mut one_u64 = [0u8; 8];
        let encrypted_data =
            PoseidonCipher::from_slice(&buf[0..PoseidonCipher::SIZE])?;
        let nonce = BlsScalar::from_slice(&buf[PoseidonCipher::SIZE..128])?;
        let stealth_address = StealthAddress::from_slice(&buf[128..192])?;
        let hashed_secret = BlsScalar::from_slice(&buf[192..224])?;
        let c = JubJubAffine::from_slice(&buf[224..256])?;
        // TODO: Change once https://github.com/dusk-network/dusk-bytes/issues/12 is addressed.
        one_u64[..].copy_from_slice(&buf[256..264]);
        let eligibility = u64::from_le_bytes(one_u64);
        one_u64[..].copy_from_slice(&buf[264..272]);
        let expiration = u64::from_le_bytes(one_u64);
        one_u64[..].copy_from_slice(&buf[272..Self::SIZE]);
        let pos = u64::from_le_bytes(one_u64);

        Ok(Bid {
            encrypted_data,
            nonce,
            stealth_address,
            hashed_secret,
            c,
            eligibility,
            expiration,
            pos,
        })
    }

    fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..PoseidonCipher::SIZE]
            .copy_from_slice(&self.encrypted_data.to_bytes());
        buf[PoseidonCipher::SIZE..128].copy_from_slice(&self.nonce.to_bytes());
        buf[128..192].copy_from_slice(&self.stealth_address.to_bytes());
        buf[192..224].copy_from_slice(&self.hashed_secret.to_bytes());
        buf[224..256].copy_from_slice(&self.c.to_bytes());
        buf[256..264].copy_from_slice(&self.eligibility.to_le_bytes());
        buf[264..272].copy_from_slice(&self.expiration.to_le_bytes());
        buf[272..Self::SIZE].copy_from_slice(&self.pos.to_le_bytes());
        buf
    }
}

impl Eq for Bid {}

impl Bid {
    /// Generates a new [Bid](self::Bid) from a rng source plus it's fields.  
    pub fn new<R>(
        rng: &mut R,
        stealth_address: &StealthAddress,
        value: &JubJubScalar,
        secret: &JubJubAffine,
        secret_k: BlsScalar,
        eligibility: u64,
        expiration: u64,
    ) -> Result<Self, BlindBidError>
    where
        R: RngCore + CryptoRng,
    {
        // Check if the bid_value is in the correct range, otherways, fail.
        match (
            value.reduce() > crate::V_MAX.reduce(),
            value.reduce() < crate::V_MIN.reduce(),
        ) {
            (true, false) => {
                return Err(BlindBidError::MaximumBidValueExceeded {
                    max_val: crate::V_MAX,
                    found: *value,
                })?;
            }
            (false, true) => {
                return Err(BlindBidError::MinimumBidValueUnreached {
                    min_val: crate::V_MIN,
                    found: *value,
                });
            }
            (false, false) => (),
            (_, _) => unreachable!(),
        };
        // Generate an empty Bid and fill it with the correct values
        let mut bid = Bid {
            // Compute and add the `hashed_secret` to the Bid.
            hashed_secret: sponge::hash(&[secret_k]),
            eligibility,
            expiration,
            c: JubJubAffine::default(),
            stealth_address: *stealth_address,
            encrypted_data: PoseidonCipher::default(),
            nonce: BlsScalar::default(),
            pos: 0u64,
        };

        bid.set_value(rng, value, secret);

        Ok(bid)
    }

    /// One-time prover-id is stated to be `H(secret_k, sigma^s, k^t, k^s)`.
    ///
    /// The function performs the sponge_hash techniqe using poseidon to
    /// get the one-time prover_id and sets it in the Bid.
    pub fn generate_prover_id(
        &self,
        secret_k: BlsScalar,
        consensus_round_seed: BlsScalar,
        latest_consensus_round: BlsScalar,
        latest_consensus_step: BlsScalar,
    ) -> BlsScalar {
        sponge::hash(&[
            secret_k,
            consensus_round_seed,
            latest_consensus_round,
            latest_consensus_step,
        ])
    }

    /// Decrypt the underlying data provided the secret of the bidder and return
    /// a tuple containing the value and the blinder fields.
    pub fn decrypt_data(
        &self,
        secret: &JubJubAffine,
    ) -> Result<(JubJubScalar, JubJubScalar), BlindBidError> {
        self.encrypted_data
            .decrypt(secret, &self.nonce)
            .map(|message| {
                let value = message[0];
                let blinder = message[1];

                let value =
                    JubJubScalar::from_raw(*value.reduce().internal_repr());
                let blinder =
                    JubJubScalar::from_raw(*blinder.reduce().internal_repr());

                (value, blinder)
            })
            .map_err(|_| BlindBidError::WrongSecretProvided)
    }

    pub(crate) fn set_value<R>(
        &mut self,
        rng: &mut R,
        value: &JubJubScalar,
        secret: &JubJubAffine,
    ) where
        R: RngCore + CryptoRng,
    {
        let blinder = JubJubScalar::random(rng);
        self.nonce = BlsScalar::random(rng);
        self.encrypted_data = PoseidonCipher::encrypt(
            &[(*value).into(), blinder.into()],
            secret,
            &self.nonce,
        );

        self.c = JubJubAffine::from(
            &(GENERATOR_EXTENDED * value)
                + &(GENERATOR_NUMS_EXTENDED * blinder),
        );
    }
}
