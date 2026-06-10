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
  username: string | null;
  uris: HostUri[];
  fields: Record<string, string>;
}

/** Spawn `bw serve` on a loopback ephemeral port. */
export async function vaultStart(): Promise<void> {
  return invoke("vault_start");
}

/** Configure the server URL (if needed) and unlock with the master password. */
export async function vaultUnlock(
  serverUrl: string,
  masterPassword: string,
): Promise<void> {
  return invoke("vault_unlock", { serverUrl, masterPassword });
}

/** Lock the vault and zeroize the session token. */
export async function vaultLock(): Promise<void> {
  return invoke("vault_lock");
}

/** Sync, then list hosts parsed from Login items. */
export async function vaultListHosts(): Promise<Host[]> {
  return invoke("vault_list_hosts");
}
