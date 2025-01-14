// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//
// Copyright (c) DUSK NETWORK. All rights reserved.

#![allow(non_snake_case)]

use super::tree_assets::BidTree;
use crate::{Bid, BlindBidCircuit, BlindBidError, Score, V_RAW_MAX, V_RAW_MIN};
use anyhow::Result;
use canonical_host::MemStore;
use dusk_bytes::Serializable;
use dusk_pki::{PublicSpendKey, SecretSpendKey};
use dusk_plonk::jubjub::{JubJubAffine, GENERATOR_EXTENDED};
use dusk_plonk::prelude::*;
use rand::Rng;

fn random_bid(secret: &JubJubScalar, secret_k: BlsScalar) -> Bid {
    let mut rng = rand::thread_rng();
    let pk_r = PublicSpendKey::from(SecretSpendKey::new(
        JubJubScalar::one(),
        -JubJubScalar::one(),
    ));
    let stealth_addr = pk_r.gen_stealth_address(&secret);
    let secret = GENERATOR_EXTENDED * secret;
    let value: u64 = (&mut rand::thread_rng()).gen_range(V_RAW_MIN, V_RAW_MAX);
    let value = JubJubScalar::from(value);
    // Set the timestamps as the max values so the proofs do not fail for them
    // (never expired or non-elegible).
    let elegibility_ts = u64::MAX;
    let expiration_ts = u64::MAX;

    Bid::new(
        &mut rng,
        &stealth_addr,
        &value,
        &secret.into(),
        secret_k,
        elegibility_ts,
        expiration_ts,
    )
    .expect("Bid creation error")
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    #[test]
    fn correct_blindbid_proof() -> Result<()> {
        // Generate Composer & Public Parameters
        let pub_params =
            PublicParameters::setup(1 << 17, &mut rand::thread_rng())?;

        // Generate a BidTree and append the Bid.
        let mut tree = BidTree::<MemStore>::new();

        // Generate a correct Bid
        let secret = JubJubScalar::random(&mut rand::thread_rng());
        let secret_k = BlsScalar::random(&mut rand::thread_rng());
        let bid = random_bid(&secret, secret_k);
        let secret: JubJubAffine = (GENERATOR_EXTENDED * &secret).into();
        // Generate fields for the Bid & required by the compute_score
        let consensus_round_seed = BlsScalar::random(&mut rand::thread_rng());
        let latest_consensus_round = 50u64;
        let latest_consensus_step = 50u64;

        // Append the Bid to the tree.
        tree.push(bid.into());

        // Extract the branch
        let branch = tree
            .poseidon_branch(0usize)
            .expect("Poseidon Branch Extraction");

        // Generate a `Score` for our Bid with the consensus parameters
        let score = Score::compute(
            &bid,
            &secret,
            secret_k,
            *branch.root(),
            consensus_round_seed,
            latest_consensus_round,
            latest_consensus_step,
        )
        .expect("Score computation error");

        let prover_id = bid.generate_prover_id(
            secret_k,
            BlsScalar::from(consensus_round_seed),
            BlsScalar::from(latest_consensus_round),
            BlsScalar::from(latest_consensus_step),
        );

        let mut circuit = BlindBidCircuit {
            bid,
            score,
            secret_k,
            secret,
            seed: BlsScalar::from(consensus_round_seed),
            latest_consensus_round: BlsScalar::from(latest_consensus_round),
            latest_consensus_step: BlsScalar::from(latest_consensus_step),
            branch: &branch,
            trim_size: 1 << 15,
            pi_positions: vec![],
        };

        let (pk, vk) = circuit
            .compile(&pub_params)
            .expect("Circuit compilation Error");
        let proof = circuit.gen_proof(&pub_params, &pk, b"CorrectBid")?;
        let storage_bid = bid.hash();
        let pi = vec![
            PublicInput::BlsScalar(*branch.root(), 0),
            PublicInput::BlsScalar(storage_bid, 0),
            PublicInput::AffinePoint(bid.commitment(), 0, 0),
            PublicInput::BlsScalar(bid.hashed_secret(), 0),
            PublicInput::BlsScalar(prover_id, 0),
            PublicInput::BlsScalar(score.value(), 0),
        ];

        let mut circuit = BlindBidCircuit {
            bid,
            score: Score::default(),
            secret_k: BlsScalar::one(),
            secret: JubJubAffine::default(),
            seed: BlsScalar::from(consensus_round_seed),
            latest_consensus_round: BlsScalar::from(latest_consensus_round),
            latest_consensus_step: BlsScalar::from(latest_consensus_step),
            branch: &branch,
            trim_size: 1 << 15,
            pi_positions: vec![],
        };
        circuit.verify_proof(&pub_params, &vk, b"CorrectBid", &proof, &pi)
    }

    #[test]
    fn edited_score_blindbid_proof() -> Result<()> {
        // Generate Composer & Public Parameters
        let pub_params =
            PublicParameters::setup(1 << 17, &mut rand::thread_rng())?;

        // Generate a BidTree and append the Bid.
        let mut tree = BidTree::<MemStore>::new();

        // Generate a correct Bid
        let secret = JubJubScalar::random(&mut rand::thread_rng());
        let secret_k = BlsScalar::random(&mut rand::thread_rng());
        let bid = random_bid(&secret, secret_k);
        let secret: JubJubAffine = (GENERATOR_EXTENDED * &secret).into();
        // Generate fields for the Bid & required by the compute_score
        let consensus_round_seed = BlsScalar::random(&mut rand::thread_rng());
        let latest_consensus_round = 50u64;
        let latest_consensus_step = 50u64;

        // Append the Bid to the tree.
        tree.push(bid.into());

        // Extract the branch
        let branch = tree
            .poseidon_branch(0usize)
            .expect("Poseidon Branch Extraction");

        // Generate a `Score` for our Bid with the consensus parameters
        let mut score = Score::compute(
            &bid,
            &secret,
            secret_k,
            *branch.root(),
            consensus_round_seed,
            latest_consensus_round,
            latest_consensus_step,
        )
        .expect("Score computation error");

        // Edit the Score so that we try to get a bigger one than the one we
        // should have got.
        score.value = score.value() + BlsScalar::from(100u64);
        let prover_id = bid.generate_prover_id(
            secret_k,
            BlsScalar::from(consensus_round_seed),
            BlsScalar::from(latest_consensus_round),
            BlsScalar::from(latest_consensus_step),
        );

        let mut circuit = BlindBidCircuit {
            bid,
            score,
            secret_k,
            secret,
            seed: BlsScalar::from(consensus_round_seed),
            latest_consensus_round: BlsScalar::from(latest_consensus_round),
            latest_consensus_step: BlsScalar::from(latest_consensus_step),
            branch: &branch,
            trim_size: 1 << 15,
            pi_positions: vec![],
        };

        let (pk, vk) = circuit
            .compile(&pub_params)
            .expect("Circuit compilation Error");
        let proof =
            circuit.gen_proof(&pub_params, &pk, b"BidWithEditedScore")?;
        let storage_bid = bid.hash();
        let pi = vec![
            PublicInput::BlsScalar(*branch.root(), 0),
            PublicInput::BlsScalar(storage_bid, 0),
            PublicInput::AffinePoint(bid.c, 0, 0),
            PublicInput::BlsScalar(bid.hashed_secret(), 0),
            PublicInput::BlsScalar(prover_id, 0),
            PublicInput::BlsScalar(score.value(), 0),
        ];
        assert!(circuit
            .verify_proof(&pub_params, &vk, b"BidWithEditedScore", &proof, &pi)
            .is_err());
        Ok(())
    }

    #[test]
    fn edited_bid_value_blindbid_proof() -> Result<()> {
        // Generate Composer & Public Parameters
        let pub_params =
            PublicParameters::setup(1 << 17, &mut rand::thread_rng())?;

        // Generate a BidTree and append the Bid.
        let mut tree = BidTree::<MemStore>::new();

        // Generate a correct Bid
        let secret = JubJubScalar::random(&mut rand::thread_rng());
        let secret_k = BlsScalar::random(&mut rand::thread_rng());
        let mut bid = random_bid(&secret, secret_k);
        let secret: JubJubAffine = (GENERATOR_EXTENDED * &secret).into();
        // Generate fields for the Bid & required by the compute_score
        let bid_tree_root = BlsScalar::random(&mut rand::thread_rng());
        let consensus_round_seed = BlsScalar::random(&mut rand::thread_rng());
        let latest_consensus_round = 25519u64;
        let latest_consensus_step = 25519u64;

        // Append the Bid to the tree.
        tree.push(bid.into());

        // Extract the branch
        let branch = tree
            .poseidon_branch(0usize)
            .expect("Poseidon Branch Extraction");

        // Generate a `Score` for our Bid with the consensus parameters
        let score = Score::compute(
            &bid,
            &secret,
            secret_k,
            bid_tree_root,
            consensus_round_seed,
            latest_consensus_round,
            latest_consensus_step,
        )
        .expect("Score computation error");

        let prover_id = bid.generate_prover_id(
            secret_k,
            BlsScalar::from(consensus_round_seed),
            BlsScalar::from(latest_consensus_round),
            BlsScalar::from(latest_consensus_step),
        );

        // Edit the Bid in order to cheat and get a bigger Score/whatever.
        bid.hashed_secret = BlsScalar::from(63463245u64);

        let mut circuit = BlindBidCircuit {
            bid,
            score,
            secret_k,
            secret,
            seed: BlsScalar::from(consensus_round_seed),
            latest_consensus_round: BlsScalar::from(latest_consensus_round),
            latest_consensus_step: BlsScalar::from(latest_consensus_step),
            branch: &branch,
            trim_size: 1 << 15,
            pi_positions: vec![],
        };

        let (pk, vk) = circuit
            .compile(&pub_params)
            .expect("Circuit compilation Error");
        let proof = circuit.gen_proof(&pub_params, &pk, b"EditedBidValue")?;
        let storage_bid = bid.hash();
        let pi = vec![
            PublicInput::BlsScalar(*branch.root(), 0),
            PublicInput::BlsScalar(storage_bid, 0),
            PublicInput::AffinePoint(bid.c, 0, 0),
            PublicInput::BlsScalar(bid.hashed_secret(), 0),
            PublicInput::BlsScalar(prover_id, 0),
            PublicInput::BlsScalar(score.value(), 0),
        ];
        assert!(circuit
            .verify_proof(&pub_params, &vk, b"EditedBidValue", &proof, &pi)
            .is_err());
        Ok(())
    }

    #[test]
    fn expired_bid_proof() -> Result<()> {
        // Generate Composer & Public Parameters
        let pub_params =
            PublicParameters::setup(1 << 17, &mut rand::thread_rng())?;

        // Generate a BidTree and append the Bid.
        let mut tree = BidTree::<MemStore>::new();

        // Create an expired bid.
        let mut rng = rand::thread_rng();
        let secret = JubJubScalar::random(&mut rng);
        let pk_r = PublicSpendKey::from(SecretSpendKey::random(&mut rng));
        let stealth_addr = pk_r.gen_stealth_address(&secret);
        let secret = JubJubAffine::from(GENERATOR_EXTENDED * secret);
        let secret_k = BlsScalar::random(&mut rng);
        let value: u64 =
            (&mut rand::thread_rng()).gen_range(V_RAW_MIN, V_RAW_MAX);
        let value = JubJubScalar::from(value);
        let expiration_ts = 100u64;
        let elegibility_ts = 1000u64;
        let bid = Bid::new(
            &mut rng,
            &stealth_addr,
            &value,
            &secret.into(),
            secret_k,
            elegibility_ts,
            expiration_ts,
        )
        .expect("Bid creation error");

        // Append the Bid to the tree.
        tree.push(bid.into());

        // Extract the branch
        let branch = tree
            .poseidon_branch(0usize)
            .expect("Poseidon Branch Extraction");

        // We first generate the score as if the bid wasn't expired. Otherways
        // the score generation would fail since the Bid would be expired.
        let latest_consensus_round = 3u64;
        let latest_consensus_step = 1u64;
        let consensus_round_seed = BlsScalar::random(&mut rand::thread_rng());

        // Generate a `Score` for our Bid with the consensus parameters
        let score = Score::compute(
            &bid,
            &secret,
            secret_k,
            *branch.root(),
            consensus_round_seed,
            latest_consensus_round,
            latest_consensus_step,
        )
        .expect("Score computation error");

        // Latest consensus step should be lower than the expiration_ts, in this
        // case is not so the proof should fail since the Bid is expired
        // at this round.
        let latest_consensus_round = 200u64;

        let prover_id = bid.generate_prover_id(
            secret_k,
            BlsScalar::from(consensus_round_seed),
            BlsScalar::from(latest_consensus_round),
            BlsScalar::from(latest_consensus_step),
        );

        let mut circuit = BlindBidCircuit {
            bid,
            score,
            secret_k,
            secret,
            seed: BlsScalar::from(consensus_round_seed),
            latest_consensus_round: BlsScalar::from(latest_consensus_round),
            latest_consensus_step: BlsScalar::from(latest_consensus_step),
            branch: &branch,
            trim_size: 1 << 15,
            pi_positions: vec![],
        };

        let (pk, vk) = circuit
            .compile(&pub_params)
            .expect("Circuit compilation Error");
        let proof = circuit.gen_proof(&pub_params, &pk, b"ExpiredBid")?;
        let storage_bid = bid.hash();
        let pi = vec![
            PublicInput::BlsScalar(*branch.root(), 0),
            PublicInput::BlsScalar(storage_bid, 0),
            PublicInput::AffinePoint(bid.c, 0, 0),
            PublicInput::BlsScalar(bid.hashed_secret(), 0),
            PublicInput::BlsScalar(prover_id, 0),
            PublicInput::BlsScalar(score.value(), 0),
        ];
        assert!(circuit
            .verify_proof(&pub_params, &vk, b"ExpiredBid", &proof, &pi)
            .is_err());
        Ok(())
    }

    #[test]
    fn non_elegible_bid() -> Result<()> {
        // Generate Composer & Public Parameters
        let pub_params =
            PublicParameters::setup(1 << 17, &mut rand::thread_rng())?;

        // Generate a BidTree and append the Bid.
        let mut tree = BidTree::<MemStore>::new();

        // Create a non-elegible Bid.
        let mut rng = rand::thread_rng();
        let secret = JubJubScalar::random(&mut rng);
        let pk_r = PublicSpendKey::from(SecretSpendKey::random(&mut rng));
        let stealth_addr = pk_r.gen_stealth_address(&secret);
        let secret = JubJubAffine::from(GENERATOR_EXTENDED * secret);
        let secret_k = BlsScalar::random(&mut rng);
        let value: u64 =
            (&mut rand::thread_rng()).gen_range(V_RAW_MIN, V_RAW_MAX);
        let value = JubJubScalar::from(value);
        let expiration_ts = 100u64;
        let elegibility_ts = 1000u64;
        let bid = Bid::new(
            &mut rng,
            &stealth_addr,
            &value,
            &secret.into(),
            secret_k,
            elegibility_ts,
            expiration_ts,
        )
        .expect("Bid creation error");

        // Append the Bid to the tree.
        tree.push(bid.into());

        // Extract the branch
        let branch = tree
            .poseidon_branch(0usize)
            .expect("Poseidon Branch Extraction");

        // We first generate the score as if the bid was still eligible.
        // Otherways the score generation would fail since the Bid
        // wouldn't be elegible.
        let latest_consensus_round = 3u64;
        let latest_consensus_step = 1u64;
        let consensus_round_seed = BlsScalar::random(&mut rand::thread_rng());

        // Generate a `Score` for our Bid with the consensus parameters
        let score = Score::compute(
            &bid,
            &secret,
            secret_k,
            *branch.root(),
            consensus_round_seed,
            latest_consensus_round,
            latest_consensus_step,
        )
        .expect("Score computation error");

        let prover_id = bid.generate_prover_id(
            secret_k,
            BlsScalar::from(consensus_round_seed),
            BlsScalar::from(latest_consensus_round),
            BlsScalar::from(latest_consensus_step),
        );

        // Latest consensus step should be lower than the elegibility_ts, in
        // this case is not so the proof should fail since the Bid is
        // non elegible anymore.
        let latest_consensus_round = 200u64;

        let mut circuit = BlindBidCircuit {
            bid,
            score,
            secret_k,
            secret,
            seed: BlsScalar::from(consensus_round_seed),
            latest_consensus_round: BlsScalar::from(latest_consensus_round),
            latest_consensus_step: BlsScalar::from(latest_consensus_step),
            branch: &branch,
            trim_size: 1 << 15,
            pi_positions: vec![],
        };

        let (pk, vk) = circuit
            .compile(&pub_params)
            .expect("Circuit compilation Error");
        let proof = circuit.gen_proof(&pub_params, &pk, b"NonElegibleBid")?;
        let storage_bid = bid.hash();
        let pi = vec![
            PublicInput::BlsScalar(*branch.root(), 0),
            PublicInput::BlsScalar(storage_bid, 0),
            PublicInput::AffinePoint(bid.c, 0, 0),
            PublicInput::BlsScalar(bid.hashed_secret(), 0),
            PublicInput::BlsScalar(prover_id, 0),
            PublicInput::BlsScalar(score.value(), 0),
        ];
        assert!(circuit
            .verify_proof(&pub_params, &vk, b"NonElegibleBid", &proof, &pi)
            .is_err());
        Ok(())
    }
}

#[cfg(test)]
mod serialization_tests {
    use super::*;
    use core::result::Result;
    #[test]
    fn from_to_bytes_impl_works() -> Result<(), BlindBidError> {
        let bid = random_bid(&JubJubScalar::one(), BlsScalar::one());
        let bid_hash = bid.hash();
        let bytes = bid.to_bytes();
        let bid2 = Bid::from_bytes(&bytes)?;
        let bid_hash_2 = bid2.hash();
        assert_eq!(bid_hash.0, bid_hash_2.0);
        Ok(())
    }
}
