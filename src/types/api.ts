export type ProfileType = "vanilla" | "fabric";
export type InstallScope = "profile" | "world";
export type ContentType =
  | "mod"
  | "resource_pack"
  | "shader_pack"
  | "datapack"
  | "modpack";

export interface ProfileSummary {
  id: string;
  name: string;
  profileType: ProfileType;
  minecraftVersion: string;
  loaderVersion: string | null;
  directoryPath: string;
  accountId: string | null;
  javaPath: string | null;
  memoryMinMb: number | null;
  memoryMaxMb: number | null;
  jvmArgs: string;
  launchArgs: string;
  notes: string | null;
  createdAt: string;
  updatedAt: string;
  lastPlayedAt: string | null;
}

export interface ProfileDetail extends ProfileSummary {
  launcherDirectory: string;
  minecraftDirectory: string;
}

export interface CreateProfileInput {
  name: string;
  profileType: ProfileType;
  minecraftVersion: string;
  loaderVersion?: string | null;
  accountId?: string | null;
  javaPath?: string | null;
  memoryMinMb?: number | null;
  memoryMaxMb?: number | null;
  jvmArgs?: string;
  launchArgs?: string;
  notes?: string | null;
}

export interface DuplicateProfileInput {
  sourceProfileId: string;
  newName: string;
}

export interface LaunchPlan {
  profileId: string;
  javaExecutable: string;
  workingDirectory: string;
  gameVersion: string;
  loaderVersion: string | null;
  memoryArgs: string[];
  jvmArgs: string[];
  gameArgs: string[];
  libraries: string[];
  assetsRoot: string;
  nativesDir: string | null;
  accountId: string | null;
  mainClass: string;
  classpath: string[];
  clientJar: string;
  loggingConfig: string | null;
  username: string;
  userType: string;
  online: boolean;
  commandPreview: string[];
}

export interface LaunchHistoryEntry {
  id: string;
  profileId: string;
  accountId: string | null;
  startedAt: string;
  endedAt: string | null;
  status: "success" | "failure" | "running";
  logPath: string;
  exitCode: number | null;
}

export interface ContentSearchResult {
  projectId: string;
  slug: string;
  title: string;
  summary: string;
  author: string | null;
  iconUrl: string | null;
  contentType: ContentType;
  supportedVersions: string[];
  supportedLoaders: string[];
  categories: string[];
}

export interface ModrinthSearchInput {
  query: string;
  profileId?: string | null;
  contentType?: ContentType | null;
}

export interface DependencyWarning {
  projectId: string;
  versionId: string | null;
  kind: "required" | "optional" | "incompatible";
  reason: string;
}

export interface InstallPlan {
  profileId: string;
  projectTitle: string;
  versionLabel: string;
  contentType: ContentType;
  installScope: InstallScope;
  projectId: string;
  versionId: string;
  targetRelPath: string | null;
  targetPath: string;
  rollbackPath: string | null;
  dependencies: DependencyWarning[];
  compatibilityWarnings: string[];
}

export interface CreateInstallPlanInput {
  profileId: string;
  projectId: string;
  contentType: ContentType;
  installScope?: InstallScope | null;
  targetRelPath?: string | null;
}

export interface ImportMrpackInput {
  sourcePath: string;
  newName?: string | null;
}

export interface InstallModpackInput {
  projectId: string;
  newName?: string | null;
}

export interface InstalledContentRecord {
  id: string;
  profileId: string;
  contentType: ContentType;
  installScope: InstallScope;
  provider: string;
  projectId: string;
  versionId: string;
  slug: string;
  name: string;
  localFilePath: string;
  targetRelPath: string | null;
  fileHash: string | null;
  enabled: boolean;
  versionNumber: string | null;
  installedAt: string;
  updatedAt: string;
}

export interface ToggleInstalledContentInput {
  installedContentId: string;
  enabled: boolean;
}

export interface UpdateCandidate {
  installedContentId: string;
  profileId: string;
  projectId: string;
  currentVersionId: string;
  targetVersionId: string;
  currentVersionLabel: string | null;
  targetVersionLabel: string | null;
  changelog: string | null;
  compatibilityNotes: string[];
}

export interface AccountSummary {
  id: string;
  username: string;
  uuid: string;
  provider: string;
  avatarUrl: string | null;
  currentSkinId: string | null;
  ownsMinecraft: boolean;
  ownershipVerifiedAt: string | null;
  createdAt: string;
  updatedAt: string;
  isAuthenticated: boolean;
}

export interface MinecraftVersionSummary {
  id: string;
  kind: string;
  releaseTime: string;
}

export interface FabricLoaderSummary {
  version: string;
  stable: boolean;
}

export interface CreateLocalAccountInput {
  username: string;
  uuid?: string | null;
  provider?: string | null;
}

export interface SkinEntry {
  id: string;
  localFilePath: string;
  displayName: string;
  modelVariant: "classic" | "slim";
  tags: string[];
  thumbnailPath: string | null;
  previewDataUrl: string | null;
  importedAt: string;
  updatedAt: string;
}

export interface ImportSkinInput {
  sourcePath?: string | null;
  fileName?: string | null;
  sourceBytes?: number[] | null;
  displayName?: string | null;
  modelVariant: "classic" | "slim";
  tags: string[];
}

export interface SharedProfileManifest {
  exportVersion: number;
  profileName: string;
  profileType: ProfileType;
  minecraftVersion: string;
  loaderVersion: string | null;
  javaPath: string | null;
  memoryMinMb: number | null;
  memoryMaxMb: number | null;
  jvmArgs: string;
  launchArgs: string;
  notes: string | null;
  content: Array<{
    projectId: string;
    versionId: string;
    contentType: ContentType;
    installScope: InstallScope;
    targetRelPath: string | null;
  }>;
}

export interface ShareExportResult {
  shareCode: string;
  exportPath: string;
  manifest: SharedProfileManifest;
}

export interface ImportShareFileInput {
  sourcePath: string;
  newName?: string | null;
}

export interface SettingEntry {
  key: string;
  value: string;
  category: string;
  updatedAt: string;
}

export interface DashboardSnapshot {
  profileCount: number;
  vanillaProfileCount: number;
  fabricProfileCount: number;
  latestProfileName: string | null;
  signedInAccountCount: number;
  launcherUnlocked: boolean;
  localSkinCount: number;
  pendingUpdateCount: number;
}
