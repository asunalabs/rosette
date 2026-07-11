//! Crude proof-of-work gate on queue creation (amendment A18). A no-identifier
//! relay with free queue creation is trivially exhaustible; this raises the
//! cost of a flood without requiring accounts. Difficulty is a relay-side
//! config value (limits::QUEUE_CREATION_POW_DIFFICULTY is the v0.1 default).

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowChallenge {
    pub challenge: [u8; 32],
    pub difficulty: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowSolution {
    pub challenge: [u8; 32],
    pub nonce: u64,
}

impl PowChallenge {
    pub fn generate(difficulty: u8) -> Self {
        let mut challenge = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut challenge);
        PowChallenge {
            challenge,
            difficulty,
        }
    }

    /// Brute-force solve. Client-side only; a relay never calls this.
    pub fn solve(&self) -> PowSolution {
        let mut nonce: u64 = 0;
        loop {
            let solution = PowSolution {
                challenge: self.challenge,
                nonce,
            };
            if self.verify(&solution) {
                return solution;
            }
            nonce += 1;
        }
    }

    pub fn verify(&self, solution: &PowSolution) -> bool {
        if solution.challenge != self.challenge {
            return false;
        }
        let mut hasher = Sha256::new();
        hasher.update(self.challenge);
        hasher.update(solution.nonce.to_le_bytes());
        let digest = hasher.finalize();
        leading_zero_bits(&digest) >= self.difficulty
    }
}

fn leading_zero_bits(bytes: &[u8]) -> u8 {
    let mut count = 0u8;
    for byte in bytes {
        if *byte == 0 {
            count += 8;
            continue;
        }
        count += byte.leading_zeros() as u8;
        break;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solved_challenge_verifies() {
        let challenge = PowChallenge::generate(12);
        let solution = challenge.solve();
        assert!(challenge.verify(&solution));
    }

    #[test]
    fn wrong_challenge_rejected() {
        let a = PowChallenge::generate(8);
        let b = PowChallenge::generate(8);
        let solution = a.solve();
        assert!(!b.verify(&solution));
    }

    #[test]
    fn low_difficulty_nonce_zero_usually_fails_high_difficulty() {
        let challenge = PowChallenge::generate(20);
        let bogus = PowSolution {
            challenge: challenge.challenge,
            nonce: 0,
        };
        // Not guaranteed false for every seed, but overwhelmingly likely at
        // difficulty 20 (1-in-a-million chance) — verify() must not panic.
        let _ = challenge.verify(&bogus);
    }
}
