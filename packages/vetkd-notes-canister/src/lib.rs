#[cfg(not(debug_assertions))]
include!(concat!(env!("OUT_DIR"), "/canister_ids.rs"));

// Provide a default for Rust Analyzer to avoid errors
#[cfg(debug_assertions)]
pub const VETKD_SYSTEM_API_CANISTER_ID: &str = "mock-canister-id"; // Temporary placeholder

use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use ic_cdk_macros::*;
use ic_certified_map::RbTree;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{
    storable::Bound, DefaultMemoryImpl, StableBTreeMap, StableCell, Storable,
};
use ic_vetkd_notes::{EncryptedNote, NoteId, EVERYONE};
use serde::Serialize;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::str::FromStr;
type Memory = VirtualMemory<DefaultMemoryImpl>;
use animals::Animal;
use asset_util::CertifiedAssets;
use ic_canister_sig_creation::signature_map::SignatureMap;
use ic_cdk::api::set_certified_data;
use ic_certified_map::AsHashTree;

mod animals;
mod certified_data;
mod http;
mod service;

#[derive(CandidType, Deserialize, Default)]
pub struct NoteIds {
    ids: Vec<NoteId>,
}

impl NoteIds {
    pub fn iter(&self) -> impl std::iter::Iterator<Item = &NoteId> {
        self.ids.iter()
    }
}

impl Storable for NoteIds {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }
    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
    const BOUND: Bound = Bound::Unbounded;
}

// We use a canister's stable memory as storage. This simplifies the code and makes the appliation
// more robust because no (potentially failing) pre_upgrade/post_upgrade hooks are needed.
// Note that stable memory is less performant than heap memory, however.
// Currently, a single canister smart contract is limited to 96 GB of stable memory.
// For the current limits see https://internetcomputer.org/docs/current/developer-docs/production/resource-limits.
// To ensure that our canister does not exceed the limit, we put various restrictions (e.g., number of users) in place.
static MAX_USERS: u64 = 1_000;
static MAX_NOTES_PER_USER: usize = 50;
static MAX_NOTE_CHARS: usize = 100000;
static MAX_SHARES_PER_NOTE: usize = 50;

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
        RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

    static NEXT_NOTE_ID: RefCell<StableCell<NoteId, Memory>> = RefCell::new(
        StableCell::init(
            MEMORY_MANAGER.with_borrow(|m| m.get(MemoryId::new(0))),
            1
        ).expect("failed to init NEXT_NOTE_ID")
    );

    static NOTES: RefCell<StableBTreeMap<NoteId, EncryptedNote, Memory>> = RefCell::new(
        StableBTreeMap::init(
            MEMORY_MANAGER.with_borrow(|m| m.get(MemoryId::new(1))),
        )
    );

    static NOTE_OWNERS: RefCell<StableBTreeMap<String, NoteIds, Memory>> = RefCell::new(
        StableBTreeMap::init(
            MEMORY_MANAGER.with_borrow(|m| m.get(MemoryId::new(2))),
        )
    );

    static NOTE_SHARES: RefCell<StableBTreeMap<String, NoteIds, Memory>> = RefCell::new(
        StableBTreeMap::init(
            MEMORY_MANAGER.with_borrow(|m| m.get(MemoryId::new(3))),
        )
    );

    static ANIMALS: RefCell<HashMap<u32, Animal>> = RefCell::new(HashMap::new());
    static SIGNATURES : RefCell<SignatureMap> = RefCell::new(SignatureMap::default());
    static ASSETS: RefCell<CertifiedAssets> = RefCell::new(CertifiedAssets::default());

}

/// Unlike Motoko, the caller identity is not built into Rust.
/// Thus, we use the ic_cdk::caller() method inside this wrapper function.
/// The wrapper prevents the use of the anonymous identity. Forbidding anonymous
/// interactions is the recommended default behavior for IC canisters.
fn caller() -> Principal {
    let caller = ic_cdk::caller();
    // The anonymous principal is not allowed to interact with the
    // encrypted notes canister.
    if caller == Principal::anonymous() {
        panic!("Anonymous principal not allowed to make calls.")
    }
    caller
}

/// --- Queries vs. Updates ---
///
/// Note that our public methods are declared as an *updates* rather than *queries*, e.g.:
/// #[update(name = "notesCnt")] ...
/// rather than
/// #[query(name = "notesCnt")] ...
///
/// While queries are significantly faster than updates, they are not certified by the IC.
/// Thus, we avoid using queries throughout this dapp, ensuring that the result of our
/// methods gets through consensus. Otherwise, this method could e.g. omit some notes
/// if it got executed by a malicious node. (To make the dapp more efficient, one could
/// use an approach in which both queries and updates are combined.)
///
/// See https://internetcomputer.org/docs/current/concepts/canisters-code#query-and-update-methods

/// Reflects the [caller]'s identity by returning (a future of) its principal.
/// Useful for debugging.
#[update]
fn whoami() -> String {
    ic_cdk::caller().to_string()
}

/// General assumptions
/// -------------------
/// All the functions of this canister's public API should be available only to
/// registered users, with the exception of [whoami].

#[update]
fn refresh_note(note_id: NoteId) -> EncryptedNote {
    let note = NOTES.with_borrow(|notes| {
        notes
            .get(&note_id)
            .unwrap_or_else(|| ic_cdk::trap("note not found"))
    });
    if !note.is_authorized() {
        ic_cdk::trap("unauthorized refresh");
    }
    note
}

/// Returns (a future of) this [caller]'s notes.
/// Panics:
///     [caller] is the anonymous identity
#[update]
fn get_notes() -> Vec<EncryptedNote> {
    let user_str = caller().to_string();
    NOTES.with_borrow(|notes| {
        let mut result = HashMap::<NoteId, EncryptedNote>::new();
        let owned = NOTE_OWNERS.with_borrow(|ids| {
            ids.get(&user_str)
                .unwrap_or_default()
                .iter()
                .map(|id| notes.get(id).ok_or(format!("missing note with ID {id}")))
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_else(|err: String| ic_cdk::trap(&err))
        });
        let shared = NOTE_SHARES.with_borrow(|ids| {
            ids.get(&user_str)
                .unwrap_or_default()
                .iter()
                .map(|id| notes.get(id).ok_or(format!("missing note with ID {id}")))
                .filter(|note| {
                    if let Ok(item) = note {
                        if owned.clone().iter().any(|u| u.id() == item.id()) {
                            return false;
                        }
                        item.users().iter().any(|(u, r)| {
                            if EVERYONE != u {
                                u == &user_str
                                    && (r.when().is_none()
                                        || r.when().unwrap() <= ic_cdk::api::time())
                            } else {
                                r.when().is_none() || r.when().unwrap() <= ic_cdk::api::time()
                            }
                        })
                    } else {
                        false
                    }
                })
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_else(|err: String| ic_cdk::trap(&err))
        });
        let public = NOTE_SHARES.with_borrow(|ids| {
            ids.get(&"everybody".to_string())
                .unwrap_or_default()
                .iter()
                .map(|id| notes.get(id).ok_or(format!("missing note with ID {id}")))
                .filter(|note| {
                    if let Ok(item) = note {
                        if owned.clone().iter().any(|u| u.id() == item.id())
                            || shared.clone().iter().any(|u| u.id() == item.id())
                        {
                            return false;
                        }
                        item.users().iter().any(|(u, r)| {
                            if EVERYONE != u {
                                u == &user_str
                                    && (r.when().is_none()
                                        || r.when().unwrap() <= ic_cdk::api::time())
                            } else {
                                r.when().is_none() || r.when().unwrap() <= ic_cdk::api::time()
                            }
                        })
                    } else {
                        false
                    }
                })
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_else(|err: String| ic_cdk::trap(&err))
        });
        // Use `extend` to add the notes to the `HashSet`
        for note in owned.iter().chain(shared.iter()).chain(public.iter()) {
            result.entry(note.id()).or_insert(note.clone());
        }

        // Convert the HashSet into a Vec to return the unique values
        let mut output: Vec<_> = result.values().cloned().collect();
        output.sort_by_key(|note| note.id());
        output
    })
}

/// Delete this [caller]'s note with given id. If none of the
/// existing notes have this id, do nothing.
/// [id]: the id of the note to be deleted
///
/// Returns:
///      Future of unit
/// Panics:
///      [caller] is the anonymous identity
///      [caller] is not the owner of note with id `note_id`
#[update]
fn delete_note(note_id: u128) {
    let user_str = caller().to_string();
    NOTES.with_borrow_mut(|notes| {
        if let Some(note_to_delete) = notes.get(&note_id) {
            let owner = &note_to_delete.owner();
            if owner != &user_str || note_to_delete.locked() {
                ic_cdk::trap("only the owner can delete unlocked notes");
            }
            NOTE_OWNERS.with_borrow_mut(|owner_to_nids| {
                if let Some(mut owner_ids) = owner_to_nids.get(owner) {
                    owner_ids.ids.retain(|&id| id != note_id);
                    if !owner_ids.ids.is_empty() {
                        owner_to_nids.insert(owner.clone(), owner_ids);
                    } else {
                        owner_to_nids.remove(owner);
                    }
                }
            });
            NOTE_SHARES.with_borrow_mut(|share_to_nids| {
                for (share_name, _) in note_to_delete.users() {
                    let share_key = share_name.to_string();
                    if let Some(mut share_ids) = share_to_nids.get(&share_key) {
                        share_ids.ids.retain(|&id| id != note_id);
                        if !share_ids.ids.is_empty() {
                            share_to_nids.insert(share_key, share_ids);
                        } else {
                            share_to_nids.remove(&share_key);
                        }
                    }
                }
            });
            notes.remove(&note_id);
        }
    });
}

/// Replaces the encrypted text of note with ID [id] with [encrypted_text].
///
/// Panics:
///     [caller] is the anonymous identity
///     [caller] is not the note's owner and not a user with whom the note is shared
///     [encrypted_text] exceeds [MAX_NOTE_CHARS]
#[update]
fn update_note(id: NoteId, data: String, encrypted_text: String) {
    NOTES.with_borrow_mut(|notes| {
        if let Some(mut note_to_update) = notes.get(&id) {
            if !note_to_update.is_authorized() || note_to_update.locked() {
                ic_cdk::trap("unauthorized update");
            }
            assert!(encrypted_text.chars().count() <= MAX_NOTE_CHARS);
            note_to_update.set_data_and_encrypted_text(data, encrypted_text);

            notes.insert(id, note_to_update);
        }
    })
}

/// Add new empty note for this [caller].
///
/// Returns:
///      Future of ID of new empty note
/// Panics:
///      [caller] is the anonymous identity
///      User already has [MAX_NOTES_PER_USER] notes
///      This is the first note for [caller] and [MAX_USERS] is exceeded
#[update]
fn create_note() -> NoteId {
    let owner = caller().to_string();

    NOTES.with_borrow_mut(|id_to_note| {
        NOTE_OWNERS.with_borrow_mut(|owner_to_nids| {
            let next_note_id = NEXT_NOTE_ID.with_borrow(|id| *id.get());
            let new_note = EncryptedNote::create(next_note_id);

            if let Some(mut owner_nids) = owner_to_nids.get(&owner) {
                assert!(owner_nids.ids.len() < MAX_NOTES_PER_USER);
                owner_nids.ids.push(new_note.id());
                owner_to_nids.insert(owner, owner_nids);
            } else {
                assert!(owner_to_nids.len() < MAX_USERS);
                owner_to_nids.insert(
                    owner,
                    NoteIds {
                        ids: vec![new_note.id()],
                    },
                );
            }
            assert_eq!(id_to_note.insert(new_note.id(), new_note), None);

            NEXT_NOTE_ID.with_borrow_mut(|next_note_id| {
                next_note_id
                    .set(next_note_id.get() + 1)
                    .unwrap_or_else(|_e| ic_cdk::trap("failed to set NEXT_NOTE_ID"))
            });
            next_note_id
        })
    })
}

/// Shares the note with ID `note_id`` with the `user`.
/// Has no effect if the note is already shared with that user.
///
/// Panics:
///      [caller] is the anonymous identity
///      [caller] is not the owner of note with id `note_id`
#[update]
fn add_user(note_id: NoteId, user: Option<String>, when: Option<u64>) {
    let caller_str = caller().to_string();
    NOTES.with_borrow_mut(|notes| {
        NOTE_SHARES.with_borrow_mut(|user_to_nids| {
            if let Some(mut note) = notes.get(&note_id) {
                let owner = &note.owner();
                if owner != &caller_str {
                    ic_cdk::trap("only the owner can share the note");
                }
                assert!(note.users().len() < MAX_SHARES_PER_NOTE);
                if note.add_reader(&user, when) {
                    notes.insert(note_id, note);
                }
                let user_name = user.unwrap_or_else(|| "everybody".to_string());
                if let Some(mut user_ids) = user_to_nids.get(&user_name) {
                    if !user_ids.ids.contains(&note_id) {
                        user_ids.ids.push(note_id);
                        user_to_nids.insert(user_name, user_ids);
                    }
                } else {
                    user_to_nids.insert(user_name, NoteIds { ids: vec![note_id] });
                }
            }
        })
    });
}

/// Unshares the note with ID `note_id`` with the `user`.
/// Has no effect if the note is not shared with that user.
///
/// Panics:
///      [caller] is the anonymous identity
///      [caller] is not the owner of note with id `note_id`
#[update]
fn remove_user(note_id: NoteId, user: Option<String>) {
    let caller_str = caller().to_string();
    NOTES.with_borrow_mut(|notes| {
        NOTE_SHARES.with_borrow_mut(|user_to_nids| {
            if let Some(mut note) = notes.get(&note_id) {
                let owner = &note.owner();
                if owner != &caller_str {
                    ic_cdk::trap("only the owner can share the note");
                }
                if note.remove_reader(&user) {
                    notes.insert(note_id, note);

                    let user_name = user.unwrap_or_else(|| "everybody".to_string());
                    if let Some(mut user_ids) = user_to_nids.get(&user_name) {
                        user_ids.ids.retain(|&id| id != note_id);
                        if !user_ids.ids.is_empty() {
                            user_to_nids.insert(user_name, user_ids);
                        } else {
                            user_to_nids.remove(&user_name);
                        }
                    }
                } else {
                    notes.insert(note_id, note);
                }
            }
        })
    });
}

mod vetkd_types;

use vetkd_types::{
    CanisterId, VetKDCurve, VetKDDeriveEncryptedKeyRequest, VetKDEncryptedKeyReply, VetKDKeyId,
    VetKDPublicKeyReply, VetKDPublicKeyRequest,
};

#[update]
async fn symmetric_key_verification_key_for_note() -> String {
    let request = VetKDPublicKeyRequest {
        canister_id: None,
        derivation_path: vec![b"note_symmetric_key".to_vec()],
        key_id: bls12_381_test_key_1(),
    };

    let (response,): (VetKDPublicKeyReply,) = ic_cdk::call(
        vetkd_system_api_canister_id(),
        "vetkd_public_key",
        (request,),
    )
    .await
    .expect("call to vetkd_public_key failed");

    hex::encode(response.public_key)
}

#[update]
async fn encrypted_symmetric_key_for_note(
    note_id: NoteId,
    encryption_public_key: Vec<u8>,
) -> String {
    let user_str = caller().to_string();
    let request = NOTES.with_borrow_mut(|notes| {
        if let Some(mut note) = notes.get(&note_id) {
            if !note.lock_authorized() {
                ic_cdk::trap(&format!("unauthorized key request by user {user_str}"));
            }
            let result = VetKDDeriveEncryptedKeyRequest {
                derivation_id: {
                    let mut buf = vec![];
                    buf.extend_from_slice(&note_id.to_be_bytes()); // fixed-size encoding
                    buf.extend_from_slice(note.owner().as_bytes());
                    buf // prefix-free
                },
                derivation_path: vec![b"note_symmetric_key".to_vec()],
                key_id: bls12_381_test_key_1(),
                encryption_public_key,
            };
            notes.insert(note_id, note.clone());
            ic_cdk::println!(
                "update note with ID {note_id} as {note:#?} retrieving from {user_str}"
            );
            result
        } else {
            ic_cdk::trap(&format!("note with ID {note_id} does not exist"));
        }
    });

    let (response,): (VetKDEncryptedKeyReply,) = ic_cdk::call(
        vetkd_system_api_canister_id(),
        "vetkd_derive_encrypted_key",
        (request,),
    )
    .await
    .expect("call to vetkd_derive_encrypted_key failed");

    hex::encode(response.encrypted_key)
}

fn bls12_381_test_key_1() -> VetKDKeyId {
    VetKDKeyId {
        curve: VetKDCurve::Bls12_381_G2,
        name: "insecure_test_key_1".to_string(),
    }
}

pub fn vetkd_system_api_canister_id() -> CanisterId {
    CanisterId::from_str(VETKD_SYSTEM_API_CANISTER_ID).expect("failed to create canister ID")
}

#[derive(CandidType, Serialize, Deserialize, Clone)]
pub struct HeaderField(pub String, pub String);

pub type StatusCode = u16;

pub type Blob = Vec<u8>;
#[derive(CandidType, Serialize, Deserialize, Debug)]
struct HttpRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    certificate_version: Option<u16>,
}

#[derive(CandidType, Serialize, Deserialize, Debug)]
struct HttpResponse {
    status_code: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

// ✅ **Certified storage** - Using a single RbTree for certification
thread_local! {
    static CERTIFIED_MESSAGES: RefCell<RbTree<Vec<u8>, [u8; 32]>> = RefCell::new(RbTree::new());
    static MESSAGE_STORAGE: RefCell<HashMap<String, (String, u16)>> = RefCell::new(HashMap::new());
    static CERTIFIED_DATA: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

// ✅ **Hashing function**
// fn hash_json(json_str: &str) -> Hash {
//     let mut hasher = Sha256::new();
//     hasher.update(json_str.as_bytes());
//     let result = hasher.finalize();
//     Hash::try_from(&result[..]).unwrap()
// }

#[update]
fn force_invalid_certification() {
    ic_cdk::println!("❌ Setting INVALID Certified Data!");
    set_certified_data(&[255; 32]); // Set an invalid hash
}

#[update]
fn update_certified_data() {
    CERTIFIED_MESSAGES.with(|storage| {
        let storage_ref = storage.borrow();
        let root_hash = storage_ref.root_hash();

        ic_cdk::println!("🟢 Calling set_certified_data() with: {:?}", root_hash);
        set_certified_data(&root_hash);
    });
}

// 🚀 **Step 1: POST Handler for Storing Data**
// #[update]
// fn store_data(body: Vec<u8>) -> String {
//     let json_request = String::from_utf8(body).unwrap_or_default();

//     CERTIFIED_DATA.with(|data| {
//         data.borrow_mut().push(json_request.clone());
//     });

//     let json_hash = hash_json(&json_request);
//     let new_entry_id = CERTIFIED_DATA.with(|data| data.borrow().len()); // Unique key

//     let key = format!("/{}", new_entry_id);

//     CERTIFIED_MESSAGES.with(|storage| {
//         let mut storage = storage.borrow_mut();
//         storage.insert(key.clone(), json_hash);
//     });

//     update_certified_data();

//     ic_cdk::println!("🚀 Stored and certified data under key: {}", key);

//     json!({
//         "success": true,
//         "entry_id": new_entry_id
//     })
//     .to_string()
// }

// fn encode_certificate(cert: &Option<Vec<u8>>) -> String {
//     cert.as_ref()
//         .map(|c| general_purpose::STANDARD.encode(c.to_bytes()))
//         .unwrap_or_default()
// }

// fn get_certified_message(message_id: &str) -> Option<(String, u16, [u8; 32])> {
//     MESSAGE_STORAGE.with(|msg_store| {
//         msg_store
//             .borrow()
//             .get(message_id)
//             .and_then(|(msg, status)| {
//                 let key = format!("/{}", message_id);
//                 CERTIFIED_MESSAGES.with(|storage| {
//                     let storage_ref = storage.borrow();
//                     let proof = storage_ref.witness(key.as_bytes()); // Use witness instead of get()
//                     ic_cdk::println!("🌳 Merkle Proof (HashTree): {:?}", proof); // Debug print HashTree
//                     match serde_cbor::to_vec(&proof) {
//                         Ok(proof_bytes) => {
//                             ic_cdk::println!("🌳 Merkle Proof (CBOR Encoded): {:?}", proof_bytes);
//                             ic_cdk::println!("🔍 Raw HashTree: {:?}", proof); // Debug print HashTree
//                             let value = (
//                                 msg.clone(),
//                                 *status,
//                                 proof_bytes.try_into().unwrap_or_default(),
//                             );
//                             ic_cdk::println!("✅ Successfully encoded Merkle Proof: {:?}", value);
//                             Some(value)
//                         }
//                         Err(err) => {
//                             ic_cdk::println!("❌ Failed to encode Merkle Proof: {:?}", err);
//                             None
//                         }
//                     }
//                 })
//             })
//     })
// }

// fn create_certified_response(message_id: &str) -> HttpResponse {
//     if let Some((message, stored_status, _hash)) = get_certified_message(message_id) {
//         let ic_certificate = data_certificate();

//         if let Some(cert) = &ic_certificate {
//             if let Ok(decoded) = serde_cbor::from_slice::<serde_cbor::Value>(cert) {
//                 ic_cdk::println!("🔍 Full Decoded Certificate: {:?}", decoded);

//                 if let serde_cbor::Value::Map(map) = decoded {
//                     if let Some(serde_cbor::Value::Bytes(certified_data_hash)) =
//                         map.get(&serde_cbor::Value::Text("tree".to_string()))
//                     {
//                         ic_cdk::println!(
//                             "✅ Extracted Certified Data Hash: {:?}",
//                             certified_data_hash
//                         );
//                     }
//                 }
//             }
//         }
//         // Create a proper witness for the message and serialize it immediately
//         let key = format!("/{}", message_id);
//         let merkle_proof = CERTIFIED_MESSAGES.with(|storage| {
//             let storage_ref = storage.borrow();
//             let tree = storage_ref.witness(key.as_bytes());
//             serde_cbor::to_vec(&tree).unwrap_or_default()
//         });

//         let mut keys: Vec<&str> = key.split('/').collect();
//         *keys.get_mut(0).unwrap() = "http_expr";
//         keys.push(".");

//         let mut expr_path_serializer = Serializer::new(vec![]);
//         expr_path_serializer.self_describe().unwrap();
//         let keys = keys.serialize(&mut expr_path_serializer).unwrap();
//         let resp = HttpResponse {
//             status_code: stored_status,
//             headers: vec![
//                 ("Content-Type".to_string(), "application/json".to_string()),
//                 (
//                     "IC-Certificate".to_string(),
//                     format!(
//                         "certificate=:{}:, tree=:{}:, expr_path=:{}:, version=2",
//                         encode_certificate(&ic_certificate),
//                         encode_certificate(&Some(merkle_proof)),
//                         encode_certificate(&Some(expr_path_serializer.into_inner())),
//                     ),
//                 ),
//                 (
//                     "IC-CertificateExpression".to_string(),
//                     "default_certification(ValidationArgs{certification:Certification{no_request_certification:Empty{},response_certification:ResponseCertification{certified_response_headers:ResponseHeaderList{headers:[{}]}}}})".to_string(),
//                 ),
//             ],
//             body: serde_json::to_vec(&json!({ "error": message })).unwrap(),
//         };
//         ic_cdk::println!(
//             "✅ Certified response created for message ID: {} {:?}",
//             message_id,
//             resp
//         );
//         resp
//     } else {
//         // Fallback if the message_id isn't found
//         ic_cdk::println!("❌ Unknown message ID: {}", message_id);
//         HttpResponse {
//             status_code: 500,
//             headers: vec![("Content-Type".to_string(), "application/json".to_string())],
//             body: serde_json::to_vec(&json!({ "error": "Unknown message ID" })).unwrap(),
//         }
//     }
// }

// 🚀 **Step 2: Query Handler for Retrieving Certified Data**
// #[query]
// fn http_request(req: HttpRequest) -> HttpResponse {
//     ic_cdk::println!("🛠 HTTP Request: {:?}", req);
//     // Always ensure we have a certificate
//     let ic_certificate = data_certificate();
//     ic_cdk::println!("🟢 IC Certificate: {:?}", ic_certificate);
//     if ic_certificate.is_none() {
//         ic_cdk::println!("❌ No data certificate available");
//         return HttpResponse {
//             status_code: 500,
//             headers: vec![("Content-Type".to_string(), "application/json".to_string())],
//             body: serde_json::to_vec(&json!({ "error": "No certificate available" })).unwrap(),
//         };
//     }

//     // ✅ Ensure it's a GET request
//     if req.method != "GET" {
//         return create_certified_response("invalid_method");
//     }

//     // ✅ Extract the path from the URL
//     let path_segments: Vec<&str> = req.url.trim_start_matches('/').split('/').collect();

//     if path_segments.len() != 2 || path_segments[0] != "data" {
//         return create_certified_response("not_found");
//     }

//     // Try to parse as a numeric entry_id first
//     if let Ok(entry_id) = path_segments[1].parse::<u64>() {
//         // Handle numeric entry_id (for stored data)
//         let json_response = CERTIFIED_DATA.with(|data| {
//             data.borrow()
//                 .get(entry_id as usize)
//                 .cloned()
//                 .unwrap_or_else(|| "Not Found".to_string())
//         });

//         if json_response == "Not Found" {
//             return create_certified_response("not_found");
//         }

//         // Create a key for this entry
//         let key = format!("/{}", entry_id);
//         let key_bytes = key.as_bytes().to_vec();

//         // Get the hash for this data
//         let hash = hash_json(&json_response);

//         // Create a witness for this key and serialize it immediately
//         let merkle_proof = CERTIFIED_MESSAGES.with(|storage| {
//             let storage_ref = storage.borrow();
//             let tree = storage_ref.witness(&key_bytes);
//             let labeled_tree = labeled(b"certified_data", tree); // Wrap with label
//             serde_cbor::to_vec(&labeled_tree).unwrap_or_default()
//         });

//         let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];

//         headers.push((
//             "IC-Certificate".to_string(),
//             encode_certificate(&ic_certificate),
//         ));

//         headers.push((
//             "IC-MerkleProof".to_string(),
//             encode_certificate(&Some(merkle_proof)),
//         ));

//         HttpResponse {
//             status_code: 200,
//             headers,
//             body: json_response.into_bytes(),
//         }
//     } else {
//         // Handle string message_id (for error messages)
//         create_certified_response("invalid_id")
//     }
// }

// ✅ Shared function to initialize certified storage
// fn initialize_certified_storage() {
//     let mut certified_map = RbTree::new();
//     let mut msg_map = HashMap::new();

//     let messages = [
//         (
//             "invalid_method",
//             "Invalid method. Only GET is allowed.",
//             405,
//         ),
//         ("invalid_format", "Invalid format.", 400),
//         ("invalid_id", "Invalid entry_id. Must be a number.", 400),
//         ("not_found", "Requested resource not found.", 404),
//         ("internal_error", "Internal server error.", 500),
//     ];

//     // Create a new HashTree that stores everything under "certified_data"
//     for (id, text, status) in messages {
//         let hash = Sha256::digest(text.as_bytes());
//         let hash_bytes: [u8; 32] = hash.into();

//         let key = format!("/{}", id);
//         certified_map.insert(key.into_bytes(), hash_bytes);
//         msg_map.insert(id.to_string(), (text.to_string(), status));
//     }

//     // Add a sample data entry for testing
//     let sample_data = "This is a sample data entry for testing";
//     let sample_hash = hash_json(sample_data);
//     certified_map.insert("/0".as_bytes().to_vec(), sample_hash);

//     CERTIFIED_DATA.with(|data| {
//         let mut data_ref = data.borrow_mut();
//         data_ref.push(sample_data.to_string());
//     });

//     CERTIFIED_MESSAGES.with(|storage| *storage.borrow_mut() = certified_map);
//     MESSAGE_STORAGE.with(|storage| *storage.borrow_mut() = msg_map);

//     update_certified_data();
//     ic_cdk::println!("✅ Certified messages initialized with sample data!");
// }

// #[query]
// fn debug_http_request() -> HttpResponse {
//     // Test with a numeric ID that should exist
//     let request = HttpRequest {
//         method: "GET".to_string(),
//         url: "/data/z".to_string(),
//         headers: vec![],
//         body: vec![],
//         certificate_version: None,
//     };
//     ic_cdk::println!("🛠 Debug HTTP Request: {:?}", request);
//     let response = http_request(request);
//     ic_cdk::println!("🛠 Debug HTTP Response: {:?}", response);

//     // Print certification information
//     ic_cdk::println!(
//         "🔍 Certificate available: {:?}",
//         data_certificate().is_some()
//     );
//     CERTIFIED_MESSAGES.with(|storage| {
//         let storage_ref = storage.borrow();
//         ic_cdk::println!("🔍 Root hash: {:?}", storage_ref.root_hash());
//         // Count the number of entries in the tree
//         let count = storage_ref.iter().count();
//         ic_cdk::println!("🔍 Tree size: {}", count);
//     });

//     response
// }

ic_cdk::export_candid!();
