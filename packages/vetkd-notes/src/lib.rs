#[cfg(not(debug_assertions))]
include!(concat!(env!("OUT_DIR"), "/canister_ids.rs"));

// Provide a default for Rust Analyzer to avoid errors
#[cfg(debug_assertions)]
pub const VETKD_SYSTEM_API_CANISTER_ID: &str = "mock-canister-id"; // Temporary placeholder

use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use custom_debug::Debug;
use ic_stable_structures::{storable::Bound, Storable};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    hash::Hash,
};

#[derive(Clone, Debug, CandidType, Deserialize, Eq, PartialEq, Hash)]
pub struct PrincipalRule {
    when: Option<u64>,
    was_read: bool,
}

impl PrincipalRule {
    pub fn when(&self) -> Option<u64> {
        self.when
    }
    pub fn was_read(&self) -> bool {
        self.was_read
    }
}

pub const EVERYONE: &str = "everyone";
pub type NoteId = u128;

#[derive(Clone, Debug, CandidType, Deserialize, Eq, PartialEq, Hash)]
pub struct HistoryEntry {
    action: String,
    user: String,
    rule: Option<(String, Option<u64>)>,
    labels: Vec<String>,
    created_at: u64,
}

impl HistoryEntry {
    pub fn action(&self) -> String {
        self.action.clone()
    }
    pub fn user(&self) -> String {
        self.user.clone()
    }
    pub fn rule(&self) -> Option<(String, Option<u64>)> {
        self.rule.clone()
    }
    pub fn created_at(&self) -> u64 {
        self.created_at
    }
    pub fn labels(&self) -> Vec<String> {
        self.labels.clone()
    }
}

#[derive(Clone, Debug, CandidType, Deserialize, Eq, PartialEq)]
pub struct EncryptedNote {
    id: NoteId,
    #[debug(skip)]
    encrypted_text: String,
    data: String,
    owner: String,
    /// Principals with whom this note is shared. Does not include the owner.
    /// Needed to be able to efficiently show in the UI with whom this note is shared.
    users: HashMap<String, PrincipalRule>,

    locked: bool,
    read_by: HashSet<String>,
    created_at: u64,
    updated_at: u64,
    history: Vec<HistoryEntry>,
}

impl Default for EncryptedNote {
    fn default() -> Self {
        EncryptedNote {
            id: 0,
            encrypted_text: "".to_string(),
            data: "".to_string(),
            owner: "".to_string(),
            users: HashMap::new(),
            locked: false,
            created_at: ic_cdk::api::time(),
            updated_at: ic_cdk::api::time(),
            history: vec![],
            read_by: HashSet::new(),
        }
    }
}

impl EncryptedNote {
    pub fn create(id: NoteId) -> Self {
        let user = &caller().to_text();
        EncryptedNote {
            id,
            owner: user.clone(),
            data: String::new(),
            users: HashMap::new(),
            encrypted_text: String::new(),
            locked: false,
            read_by: HashSet::new(),
            created_at: ic_cdk::api::time(),
            updated_at: ic_cdk::api::time(),
            history: vec![HistoryEntry {
                action: "created".to_string(),
                labels: vec![],
                user: user.clone(),
                rule: None,
                created_at: ic_cdk::api::time(),
            }],
        }
    }
    pub fn id(&self) -> NoteId {
        self.id
    }
    pub fn data(&self) -> String {
        self.data.clone()
    }
    pub fn read_by(&self) -> HashSet<String> {
        self.read_by.clone()
    }
    pub fn encrypted_text(&self) -> String {
        self.encrypted_text.clone()
    }
    pub fn owner(&self) -> String {
        self.owner.clone()
    }
    pub fn users(&self) -> HashMap<String, PrincipalRule> {
        self.users.clone()
    }
    pub fn locked(&self) -> bool {
        self.locked
    }
    pub fn created_at(&self) -> u64 {
        self.created_at
    }
    pub fn updated_at(&self) -> u64 {
        self.updated_at
    }
    pub fn history(&self) -> Vec<HistoryEntry> {
        self.history.clone()
    }
    // Check if the user is owner or has access to the note as of right now
    pub fn is_authorized(&self) -> bool {
        let user = &caller().to_text();
        if user == &self.owner {
            return true;
        }
        // once a non-owner reads a note it's locked and can no longer
        // be updated
        if let Some(r) = self.users.get(user) {
            if r.when.is_none() || r.when.unwrap() <= ic_cdk::api::time() {
                return true;
            }
        } else if let Some(r) = self.users.get(EVERYONE) {
            if r.when.is_none() || r.when.unwrap() <= ic_cdk::api::time() {
                return true;
            }
        }
        false
    }
    // Same as above but mark it as being read by that user
    pub fn lock_authorized(&mut self) -> bool {
        let user = &caller().to_text();
        if user == &self.owner {
            let id = self.id;
            ic_cdk::println!("note not locked with ID {id} as {self:#?} retrieving owner {user}");
            return true;
        }
        // once a non-owner reads a note it's locked and can no longer
        // be updated
        if let Some(r) = self.users.get_mut(user) {
            if r.when.is_none() || r.when.unwrap() <= ic_cdk::api::time() {
                r.was_read = true;
                if !self.read_by.contains(user) {
                    self.history.push(HistoryEntry {
                        action: "read".to_string(),
                        user: user.to_string(),
                        labels: if self.locked {
                            vec![]
                        } else {
                            vec!["locked".to_string()]
                        },
                        rule: Some((user.clone(), r.when)),
                        created_at: ic_cdk::api::time(),
                    });
                }
                self.locked = true;
                self.read_by.insert(user.to_string());

                let id = self.id;
                ic_cdk::println!("locked note with ID {id} as {self:#?} retrieving from {user}");

                return true;
            }
        } else if let Some(r) = self.users.get_mut(EVERYONE) {
            if r.when.is_none() || r.when.unwrap() <= ic_cdk::api::time() {
                r.was_read = true;
                if !self.read_by.contains(user) {
                    self.history.push(HistoryEntry {
                        action: "read".to_string(),
                        user: user.to_string(),
                        labels: if self.locked {
                            vec![]
                        } else {
                            vec!["locked".to_string()]
                        },
                        rule: Some((EVERYONE.to_string(), r.when)),
                        created_at: ic_cdk::api::time(),
                    });
                }
                self.read_by.insert(user.to_string());
                self.locked = true;

                let id = self.id;
                ic_cdk::println!(
                    "locked note with ID {id} as {self:#?} retrieving from {user} as everyone"
                );

                return true;
            }
        }
        false
    }
    // add a new reader to the note
    pub fn add_reader(&mut self, user: &Option<String>, when: Option<u64>) -> bool {
        if self.locked && (user.is_none() || self.read_by.contains(&user.clone().unwrap())) {
            // If this note is locked and the user has already read it then this doesn't seem useful.
            return false;
        }
        let user_name = user.clone().unwrap_or_else(|| EVERYONE.to_string());
        self.history.append(&mut vec![HistoryEntry {
            action: "share".to_string(),
            labels: vec![],
            user: user_name.clone(),
            rule: Some((user_name.clone(), when)),
            created_at: ic_cdk::api::time(),
        }]);
        self.users.insert(
            user_name,
            PrincipalRule {
                was_read: false,
                when,
            },
        );
        true
    }
    // Was the note ever read by that user
    pub fn user_read(&self, user: &String) -> bool {
        self.read_by.contains(user)
    }
    // Remove a reader (will return false if the note was already read by the user)
    pub fn remove_reader(&mut self, user: &Option<String>) -> bool {
        if self.locked {
            if user.iter().any(|u| self.read_by.contains(u)) {
                return false;
            } else if let Some(r) = self
                .users
                .get(&user.clone().unwrap_or(EVERYONE.to_string()))
            {
                if r.was_read {
                    return false;
                }
            }
        }
        let user_name = user.clone().unwrap_or_else(|| EVERYONE.to_string());
        if self.users.contains_key(&user_name) {
            self.users.remove(user_name.as_str());
            self.history.push(HistoryEntry {
                action: "unshare".to_string(),
                labels: vec![],
                user: user_name.clone(),
                rule: None,
                created_at: ic_cdk::api::time(),
            });

            true
        } else {
            false
        }
    }
    // Update the data. This is only allowed by the owner before the note was locked
    pub fn set_data(&mut self, data: String) -> bool {
        let user = caller().to_text();
        if self.locked && user != self.owner {
            return false;
        }
        self.data = data;
        self.updated_at = ic_cdk::api::time();
        self.history.push(HistoryEntry {
            action: "updated".to_string(),
            labels: vec!["data".to_string()],
            user: user.clone(),
            rule: None,
            created_at: ic_cdk::api::time(),
        });
        true
    }
    pub fn set_encrypted_text(&mut self, encrypted_text: String) -> bool {
        let user = caller().to_text();
        if self.locked && user != self.owner {
            return false;
        }
        self.encrypted_text = encrypted_text;
        self.updated_at = ic_cdk::api::time();
        self.history.push(HistoryEntry {
            action: "updated".to_string(),
            labels: vec!["encrypted_text".to_string()],
            user: user.clone(),
            rule: None,
            created_at: ic_cdk::api::time(),
        });
        true
    }
    pub fn set_data_and_encrypted_text(&mut self, data: String, encrypted_text: String) -> bool {
        let user = caller().to_text();
        if self.locked && user != self.owner {
            return false;
        }
        self.data = data;
        self.encrypted_text = encrypted_text;
        self.updated_at = ic_cdk::api::time();
        self.history.push(HistoryEntry {
            action: "updated".to_string(),
            labels: vec!["data".to_string(), "encrypted_text".to_string()],
            user: user.clone(),
            rule: None,
            created_at: ic_cdk::api::time(),
        });
        true
    }
    // Is the note shared at all?
    pub fn is_shared(&self) -> bool {
        !self.users.is_empty()
    }
    // Has any reader read it?
    pub fn is_locked(&self) -> bool {
        self.locked
    }
}

impl Storable for EncryptedNote {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }
    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
    const BOUND: Bound = Bound::Unbounded;
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

mod vetkd_types;

use vetkd_types::{
    CanisterId, VetKDCurve, VetKDEncryptedKeyReply, VetKDEncryptedKeyRequest, VetKDKeyId,
    VetKDPublicKeyReply, VetKDPublicKeyRequest,
};

pub async fn symmetric_key_verification_key_for_note() -> String {
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

// Be careful this routine will not be able to actully determine whether the
// current user is permitted to read the note. I.e. anyone can ask for any
// derivation path.
pub async fn encrypted_symmetric_key_for_note(
    note_id: NoteId,
    owner: &String,
    encryption_public_key: Vec<u8>,
) -> String {
    let request = VetKDEncryptedKeyRequest {
        derivation_id: {
            let mut buf = vec![];
            buf.extend_from_slice(&note_id.to_be_bytes()); // fixed-size encoding
            buf.extend_from_slice(owner.as_bytes());
            buf // prefix-free
        },
        public_key_derivation_path: vec![b"note_symmetric_key".to_vec()],
        key_id: bls12_381_test_key_1(),
        encryption_public_key,
    };

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
        curve: VetKDCurve::Bls12_381,
        name: "test_key_1".to_string(),
    }
}

pub fn vetkd_system_api_canister_id() -> CanisterId {
    use std::str::FromStr;
    CanisterId::from_str(VETKD_SYSTEM_API_CANISTER_ID).expect("failed to create canister ID")
}
