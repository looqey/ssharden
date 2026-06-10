// Bridge to the Rust vault commands (invoke()).
// Mirrors the `#[tauri::command]` signatures in src-tauri/src/main.rs.
// The master password is sent once to the backend; the session token never
// comes back to JS.

import { invoke } from "@tauri-apps/api/core";

/** A connection URI parsed from a Bitwarden Login item. */
export interface HostUri {
  scheme: string;
  host: string;
  port: number | null;
  user: string | null;
  raw: string;
}

/** A connectable host parsed from a Bitwarden Login item. */
export interface Host {
  id: string;
  name: string;
  folder_id: string | null;
  folder_name: string | null;
  username: string | null;
  uris: HostUri[];
  fields: Record<string, string>;
}

/** Spawn `bw serve` on a loopback ephemeral port. */
export async function vaultStart(): Promise<void> {
  return invoke("vault_start");
}

/** Unlock the already-logged-in vault with the master password. */
export async function vaultUnlock(masterPassword: string): Promise<void> {
  return invoke("vault_unlock", { serverUrl: "", masterPassword });
}

/** Auth/lock state of the bw account (which account is logged in). */
export interface AccountStatus {
  server_url: string | null;
  user_email: string | null;
  status: string; // "unauthenticated" | "locked" | "unlocked"
}

/** Read which bw account is logged in (for the unlock screen). */
export async function accountStatus(): Promise<AccountStatus> {
  return invoke("account_status");
}

/** Lock the vault and zeroize the session token. */
export async function vaultLock(): Promise<void> {
  return invoke("vault_lock");
}

/** Sync, then list hosts parsed from Login items. */
export async function vaultListHosts(): Promise<Host[]> {
  return invoke("vault_list_hosts");
}

/** User-supplied fields for creating or editing a host. */
export interface HostInput {
  name: string;
  folder_id?: string | null;
  username?: string | null;
  /** Empty/omitted on edit = keep the existing password. */
  password?: string | null;
  uris: string[];
  fields: Record<string, string>;
}

/** Create a new host (Login item). */
export async function hostCreate(input: HostInput): Promise<void> {
  return invoke("host_create", { input });
}

/** Update an existing host; blank password/folder are preserved. */
export async function hostUpdate(id: string, input: HostInput): Promise<void> {
  return invoke("host_update", { id, input });
}

/** Delete a host by id. */
export async function hostDelete(id: string): Promise<void> {
  return invoke("host_delete", { id });
}

/** Fetch a host's password for copy/reveal. */
export async function hostPassword(id: string): Promise<string | null> {
  return invoke("host_password", { id });
}

/** Launch an external RDP session (FreeRDP window) to a host's rdp:// URI. */
export async function rdpLaunch(hostId: string): Promise<void> {
  return invoke("rdp_launch", { hostId });
}

/** A vault folder (group). */
export interface Folder {
  id: string;
  name: string;
}

/** List vault folders, sorted by name. */
export async function vaultFolders(): Promise<Folder[]> {
  return invoke("vault_folders");
}

/** Create a vault folder. */
export async function folderCreate(name: string): Promise<void> {
  return invoke("folder_create", { name });
}

/** Rename a vault folder. */
export async function folderRename(id: string, name: string): Promise<void> {
  return invoke("folder_rename", { id, name });
}

/** Delete a vault folder (its items become unfiled). */
export async function folderDelete(id: string): Promise<void> {
  return invoke("folder_delete", { id });
}
