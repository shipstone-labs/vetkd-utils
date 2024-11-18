export type {
  _SERVICE,
  EncryptedNote,
  PrincipalRule,
  HistoryEntry,
} from "../../vetkd-notes-canister/vetkd_notes_canister.d.ts";
export { idlFactory } from "../../vetkd-notes-canister/vetkd_notes_canister.mjs";
export const ENCRYPTED_NOTES_CANISTER_ID =
  process.env.ENCRYPTED_NOTES_RUST_CANISTER_ID;