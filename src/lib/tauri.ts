import { invoke } from "@tauri-apps/api/core";
import type {
  AccountSummary,
  CreateInstallPlanInput,
  CreateLocalAccountInput,
  CreateProfileInput,
  DashboardSnapshot,
  FabricLoaderSummary,
  ImportMrpackInput,
  ImportShareFileInput,
  ImportSkinInput,
  InstallModpackInput,
  InstallPlan,
  InstalledContentRecord,
  LaunchHistoryEntry,
  LaunchPlan,
  MinecraftVersionSummary,
  ModrinthSearchInput,
  DuplicateProfileInput,
  ProfileDetail,
  ProfileSummary,
  SettingEntry,
  ShareExportResult,
  SkinEntry,
  ToggleInstalledContentInput,
  UpdateCandidate,
  ContentSearchResult,
} from "../types/api";

export function getDashboardSnapshot(): Promise<DashboardSnapshot> {
  return invoke("get_dashboard_snapshot");
}

export function listProfiles(): Promise<ProfileSummary[]> {
  return invoke("list_profiles");
}

export function getProfileDetail(profileId: string): Promise<ProfileDetail> {
  return invoke("get_profile_detail", { profileId });
}

export function createProfile(
  input: CreateProfileInput,
): Promise<ProfileDetail> {
  return invoke("create_profile", { input });
}

export function listMinecraftVersions(): Promise<MinecraftVersionSummary[]> {
  return invoke("list_minecraft_versions");
}

export function listFabricLoaderVersions(
  minecraftVersion: string,
): Promise<FabricLoaderSummary[]> {
  return invoke("list_fabric_loader_versions", { minecraftVersion });
}

export function resolveLaunchPlan(profileId: string): Promise<LaunchPlan> {
  return invoke("resolve_launch_plan", { profileId });
}

export function launchProfile(profileId: string): Promise<LaunchHistoryEntry> {
  return invoke("launch_profile", { profileId });
}

export function listLaunchHistory(
  profileId?: string | null,
): Promise<LaunchHistoryEntry[]> {
  return invoke("list_launch_history", { profileId: profileId ?? null });
}

export function duplicateProfile(
  input: DuplicateProfileInput,
): Promise<ProfileDetail> {
  return invoke("duplicate_profile", { input });
}

export function deleteProfile(profileId: string): Promise<void> {
  return invoke("delete_profile", { profileId });
}

export function searchModrinth(
  input: ModrinthSearchInput,
): Promise<ContentSearchResult[]> {
  return invoke("search_modrinth", { input });
}

export function createInstallPlan(
  input: CreateInstallPlanInput,
): Promise<InstallPlan> {
  return invoke("create_install_plan", { input });
}

export function applyInstallPlan(plan: InstallPlan): Promise<InstalledContentRecord> {
  return invoke("apply_install_plan", { input: { plan } });
}

export function importMrpack(
  input: ImportMrpackInput,
): Promise<ProfileDetail> {
  return invoke("import_mrpack", { input });
}

export function installModrinthModpack(
  input: InstallModpackInput,
): Promise<ProfileDetail> {
  return invoke("install_modrinth_modpack", { input });
}

export function listInstalledContent(
  profileId?: string | null,
): Promise<InstalledContentRecord[]> {
  return invoke("list_installed_content", { profileId: profileId ?? null });
}

export function toggleInstalledContent(
  input: ToggleInstalledContentInput,
): Promise<InstalledContentRecord> {
  return invoke("toggle_installed_content", { input });
}

export function removeInstalledContent(
  installedContentId: string,
): Promise<void> {
  return invoke("remove_installed_content", { installedContentId });
}

export function listUpdateCandidates(
  profileId?: string | null,
): Promise<UpdateCandidate[]> {
  return invoke("list_update_candidates", { profileId: profileId ?? null });
}

export function applyUpdateCandidate(
  installedContentId: string,
  targetVersionId: string,
): Promise<InstalledContentRecord> {
  return invoke("apply_update_candidate", {
    installedContentId,
    targetVersionId,
  });
}

export function listAccounts(): Promise<AccountSummary[]> {
  return invoke("list_accounts");
}

export function createLocalAccount(
  input: CreateLocalAccountInput,
): Promise<AccountSummary> {
  return invoke("create_local_account", { input });
}

export function signInMicrosoft(): Promise<AccountSummary> {
  return invoke("sign_in_microsoft");
}

export function deleteAccount(accountId: string): Promise<void> {
  return invoke("delete_account", { accountId });
}

export function bindProfileAccount(
  profileId: string,
  accountId?: string | null,
): Promise<void> {
  return invoke("bind_profile_account", {
    profileId,
    accountId: accountId ?? null,
  });
}

export function listSkins(): Promise<SkinEntry[]> {
  return invoke("list_skins");
}

export function importSkin(input: ImportSkinInput): Promise<SkinEntry> {
  return invoke("import_skin", { input });
}

export function deleteSkin(skinId: string): Promise<void> {
  return invoke("delete_skin", { skinId });
}

export function applySkinToAccount(
  accountId: string,
  skinId: string,
): Promise<void> {
  return invoke("apply_skin_to_account", { input: { accountId, skinId } });
}

export function exportProfileShare(profileId: string): Promise<ShareExportResult> {
  return invoke("export_profile_share", { profileId });
}

export function importProfileShare(
  shareCode: string,
  newName?: string | null,
): Promise<ProfileDetail> {
  return invoke("import_profile_share", {
    input: { shareCode, newName: newName ?? null },
  });
}

export function importProfileShareFile(
  input: ImportShareFileInput,
): Promise<ProfileDetail> {
  return invoke("import_profile_share_file", { input });
}

export function listSettings(): Promise<SettingEntry[]> {
  return invoke("list_settings");
}

export function upsertSetting(
  key: string,
  value: string,
  category: string,
): Promise<SettingEntry> {
  return invoke("upsert_setting", { key, value, category });
}
