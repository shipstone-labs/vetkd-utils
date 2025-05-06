use ic_certified_map::HashTree;
use serde_cbor::Value;
use sha2::{Digest, Sha256};

/// Verify the witness against the IC certificate
fn verify_ic_certificate(
    witness_hash: [u8; 32],
    merkle_proof_base64: &str,
    root_hash: [u8; 32],
) -> bool {
    // Step 1: Decode the base64 Merkle proof
    let merkle_proof_bytes = match base64::decode(merkle_proof_base64) {
        Ok(bytes) => bytes,
        Err(e) => {
            println!("❌ Failed to decode base64 Merkle proof: {:?}", e);
            return false;
        }
    };

    // Step 2: Deserialize CBOR into a generic Value (not HashTree)
    let cbor_value: Value = match serde_cbor::from_slice(&merkle_proof_bytes) {
        Ok(value) => value,
        Err(e) => {
            println!("❌ Failed to parse CBOR Merkle proof: {:?}", e);
            return false;
        }
    };

    // Step 3: Convert CBOR value into a HashTree manually
    let hash_tree = match cbor_to_hash_tree(&cbor_value) {
        Some(tree) => tree,
        None => {
            println!("❌ Failed to construct HashTree from CBOR");
            return false;
        }
    };

    // Step 4: Reconstruct the root hash
    let computed_root_hash = match reconstruct_root_hash(&hash_tree, witness_hash) {
        Some(hash) => hash,
        None => {
            println!("❌ Failed to reconstruct root hash");
            return false;
        }
    };

    // Step 5: Compare computed root hash with IC-Certificate root hash
    if computed_root_hash == root_hash {
        println!("✅ Root hash verified successfully!");
        true
    } else {
        println!(
            "❌ Mismatch: Computed Root Hash: {:?}, Expected: {:?}",
            computed_root_hash, root_hash
        );
        false
    }
}

/// Convert CBOR `Value` into `HashTree`
fn cbor_to_hash_tree(value: &Value) -> Option<HashTree> {
    match value {
        Value::Bytes(b) => Some(HashTree::Leaf(b.clone())),
        Value::Array(arr) if arr.len() == 2 => {
            let left = cbor_to_hash_tree(&arr[0])?;
            let right = cbor_to_hash_tree(&arr[1])?;
            Some(HashTree::Fork(Box::new((left, right))))
        }
        _ => None,
    }
}

/// Reconstruct root hash from a `HashTree`
fn reconstruct_root_hash(tree: &HashTree, witness_hash: [u8; 32]) -> Option<[u8; 32]> {
    match tree {
        HashTree::Leaf(data) => {
            if data == &witness_hash {
                Some(witness_hash)
            } else {
                None
            }
        }
        HashTree::Fork(boxed) => {
            let (left, right) = &**boxed;
            let left_hash = reconstruct_root_hash(left, witness_hash);
            let right_hash = reconstruct_root_hash(right, witness_hash);
            match (left_hash, right_hash) {
                (Some(lh), Some(rh)) => Some(labeled_hash("fork", &[lh, rh].concat())),
                (Some(lh), None) => Some(labeled_hash("fork", &[lh, [0; 32]].concat())),
                (None, Some(rh)) => Some(labeled_hash("fork", &[[0; 32], rh].concat())),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Hash function similar to ICP's labeled hashing
fn labeled_hash(label: &str, data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(label.as_bytes());
    hasher.update(data);
    let result = hasher.finalize();
    let mut array = [0; 32];
    array.copy_from_slice(&result);
    array
}
