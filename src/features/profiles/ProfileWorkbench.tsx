import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { CreateProfileInput, LaunchPlan, ProfileSummary } from "../../types/api";
import {
  createProfile,
  deleteProfile,
  duplicateProfile,
  exportProfileShare,
  importProfileShare,
  importProfileShareFile,
  launchProfile,
  listFabricLoaderVersions,
  listInstalledContent,
  listLaunchHistory,
  listMinecraftVersions,
  removeInstalledContent,
  resolveLaunchPlan,
  toggleInstalledContent,
} from "../../lib/tauri";
import { ProfileTable } from "./ProfileTable";
import { EmptyState } from "../shared/EmptyState";
import { formatDateTime } from "../../lib/format";

interface ProfileWorkbenchProps {
  profiles: ProfileSummary[];
  search: string;
}

const initialDraft: CreateProfileInput = {
  name: "",
  profileType: "vanilla",
  minecraftVersion: "",
  loaderVersion: "latest",
  notes: "",
  jvmArgs: "",
  launchArgs: "",
};

export function ProfileWorkbench({ profiles, search }: ProfileWorkbenchProps) {
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState<CreateProfileInput>(initialDraft);
  const [selectedProfileId, setSelectedProfileId] = useState("");
  const [shareCode, setShareCode] = useState("");
  const [shareFilePath, setShareFilePath] = useState("");
  const [shareImportName, setShareImportName] = useState("");
  const [lastExportPath, setLastExportPath] = useState("");
  const [resolvedPlan, setResolvedPlan] = useState<LaunchPlan | null>(null);

  const versionsQuery = useQuery({
    queryKey: ["minecraft-versions"],
    queryFn: listMinecraftVersions,
  });

  const fabricLoadersQuery = useQuery({
    queryKey: ["fabric-loaders", draft.minecraftVersion],
    queryFn: () => listFabricLoaderVersions(draft.minecraftVersion),
    enabled: draft.profileType === "fabric" && Boolean(draft.minecraftVersion),
  });

  const filteredProfiles = useMemo(() => {
    const needle = search.trim().toLowerCase();
    if (!needle) {
      return profiles;
    }

    return profiles.filter((profile) => {
      return (
        profile.name.toLowerCase().includes(needle) ||
        profile.minecraftVersion.toLowerCase().includes(needle) ||
        profile.profileType.toLowerCase().includes(needle) ||
        (profile.loaderVersion ?? "").toLowerCase().includes(needle)
      );
    });
  }, [profiles, search]);

  useEffect(() => {
    if (!selectedProfileId && profiles[0]) {
      setSelectedProfileId(profiles[0].id);
    } else if (
      selectedProfileId &&
      profiles.length > 0 &&
      !profiles.some((profile) => profile.id === selectedProfileId)
    ) {
      setSelectedProfileId(profiles[0].id);
    }
  }, [profiles, selectedProfileId]);

  useEffect(() => {
    if (!draft.minecraftVersion && versionsQuery.data?.length) {
      const defaultVersion =
        versionsQuery.data.find((version) => version.kind === "release") ??
        versionsQuery.data[0];
      setDraft((current) => ({
        ...current,
        minecraftVersion: defaultVersion.id,
      }));
    }
  }, [draft.minecraftVersion, versionsQuery.data]);

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["profiles"] }),
      queryClient.invalidateQueries({ queryKey: ["dashboard"] }),
      queryClient.invalidateQueries({ queryKey: ["installed-content"] }),
      queryClient.invalidateQueries({ queryKey: ["updates"] }),
      queryClient.invalidateQueries({ queryKey: ["launch-history"] }),
    ]);
  };

  const installedContentQuery = useQuery({
    queryKey: ["installed-content", selectedProfileId],
    queryFn: () => listInstalledContent(selectedProfileId || null),
    enabled: Boolean(selectedProfileId),
  });

  const launchHistoryQuery = useQuery({
    queryKey: ["launch-history", selectedProfileId],
    queryFn: () => listLaunchHistory(selectedProfileId || null),
    enabled: Boolean(selectedProfileId),
    refetchInterval: 5000,
  });

  const createMutation = useMutation({
    mutationFn: async () =>
      createProfile({
        ...draft,
        loaderVersion:
          draft.profileType === "fabric" ? draft.loaderVersion || "latest" : null,
        notes: draft.notes?.trim() || null,
        jvmArgs: draft.jvmArgs ?? "",
        launchArgs: draft.launchArgs ?? "",
      }),
    onSuccess: async () => {
      setDraft((current) => ({
        ...initialDraft,
        minecraftVersion: current.minecraftVersion,
      }));
      await refresh();
    },
  });

  const duplicateMutation = useMutation({
    mutationFn: (profile: ProfileSummary) =>
      duplicateProfile({
        sourceProfileId: profile.id,
        newName: `${profile.name} Copy`,
      }),
    onSuccess: refresh,
  });

  const deleteMutation = useMutation({
    mutationFn: (profileId: string) => deleteProfile(profileId),
    onSuccess: async () => {
      setResolvedPlan(null);
      await refresh();
    },
  });

  const toggleMutation = useMutation({
    mutationFn: ({
      installedContentId,
      enabled,
    }: {
      installedContentId: string;
      enabled: boolean;
    }) => toggleInstalledContent({ installedContentId, enabled }),
    onSuccess: refresh,
  });

  const removeContentMutation = useMutation({
    mutationFn: (installedContentId: string) =>
      removeInstalledContent(installedContentId),
    onSuccess: refresh,
  });

  const resolvePlanMutation = useMutation({
    mutationFn: (profileId: string) => resolveLaunchPlan(profileId),
    onSuccess: (plan) => {
      setResolvedPlan(plan);
    },
  });

  const launchMutation = useMutation({
    mutationFn: (profileId: string) => launchProfile(profileId),
    onSuccess: async () => {
      await refresh();
    },
  });

  const exportMutation = useMutation({
    mutationFn: (profileId: string) => exportProfileShare(profileId),
    onSuccess: (result) => {
      setShareCode(result.shareCode);
      setLastExportPath(result.exportPath);
    },
  });

  const importMutation = useMutation({
    mutationFn: () => importProfileShare(shareCode, shareImportName || null),
    onSuccess: async () => {
      setShareImportName("");
      await refresh();
    },
  });

  const importFileMutation = useMutation({
    mutationFn: () =>
      importProfileShareFile({
        sourcePath: shareFilePath,
        newName: shareImportName || null,
      }),
    onSuccess: async () => {
      setShareImportName("");
      setShareFilePath("");
      await refresh();
    },
  });

  const launchTarget =
    profiles.find((profile) => profile.id === selectedProfileId) ?? null;

  return (
    <div className="stack-grid">
      <div className="profiles-grid">
        <section className="panel composer-panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Create profile</p>
              <h3>Version and loader</h3>
            </div>
          </div>

          <div className="form-grid">
            <label>
              <span>Name</span>
              <input
                value={draft.name}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, name: event.target.value }))
                }
                placeholder="Speedrun Fabric"
              />
            </label>

            <label>
              <span>Profile type</span>
              <select
                value={draft.profileType}
                onChange={(event) =>
                  setDraft((current) => ({
                    ...current,
                    profileType: event.target.value as CreateProfileInput["profileType"],
                    loaderVersion:
                      event.target.value === "fabric"
                        ? current.loaderVersion || "latest"
                        : "",
                  }))
                }
              >
                <option value="vanilla">Vanilla</option>
                <option value="fabric">Fabric</option>
              </select>
            </label>

            <label>
              <span>Minecraft version</span>
              <select
                value={draft.minecraftVersion}
                onChange={(event) =>
                  setDraft((current) => ({
                    ...current,
                    minecraftVersion: event.target.value,
                  }))
                }
              >
                {(versionsQuery.data ?? []).map((version) => (
                  <option key={version.id} value={version.id}>
                    {version.id} ({version.kind})
                  </option>
                ))}
              </select>
            </label>

            <label>
              <span>Fabric loader</span>
              <select
                value={draft.loaderVersion ?? "latest"}
                disabled={draft.profileType !== "fabric"}
                onChange={(event) =>
                  setDraft((current) => ({
                    ...current,
                    loaderVersion: event.target.value,
                  }))
                }
              >
                <option value="latest">latest stable</option>
                {(fabricLoadersQuery.data ?? []).map((loader) => (
                  <option key={loader.version} value={loader.version}>
                    {loader.version}
                    {loader.stable ? " (stable)" : " (preview)"}
                  </option>
                ))}
              </select>
            </label>

            <label className="span-2">
              <span>Notes</span>
              <textarea
                rows={4}
                value={draft.notes ?? ""}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, notes: event.target.value }))
                }
                placeholder="Purpose, key mods, Java overrides, or launch notes."
              />
            </label>
          </div>

          <div className="panel-actions">
            <button
              className="primary"
              disabled={!draft.name.trim() || !draft.minecraftVersion.trim()}
              onClick={() => createMutation.mutate()}
            >
              {createMutation.isPending ? "Creating..." : "Create profile"}
            </button>
            <span className="muted">
              Choose a Minecraft version and Fabric loader here. Java is
              resolved automatically unless you pin one in Settings or the
              profile.
            </span>
          </div>

          {versionsQuery.error ? (
            <p className="error-text">{String(versionsQuery.error)}</p>
          ) : null}
          {fabricLoadersQuery.error ? (
            <p className="error-text">{String(fabricLoadersQuery.error)}</p>
          ) : null}
          {createMutation.error ? (
            <p className="error-text">{String(createMutation.error)}</p>
          ) : null}
        </section>

        <section className="panel table-panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Profiles</p>
              <h3>Profile library</h3>
            </div>
            <span className="status-pill neutral">
              {filteredProfiles.length} visible
            </span>
          </div>

          {filteredProfiles.length > 0 ? (
            <ProfileTable
              profiles={filteredProfiles}
              onDuplicate={(profile) => duplicateMutation.mutate(profile)}
              onDelete={(profile) => deleteMutation.mutate(profile.id)}
            />
          ) : (
            <EmptyState
              eyebrow="Profiles"
              title="No matching profiles"
              body="Create a new profile or widen the search filter."
            />
          )}

          {duplicateMutation.error ? (
            <p className="error-text">{String(duplicateMutation.error)}</p>
          ) : null}
          {deleteMutation.error ? (
            <p className="error-text">{String(deleteMutation.error)}</p>
          ) : null}
        </section>
      </div>

      <div className="profiles-grid secondary">
        <section className="panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Launch</p>
              <h3>Launch and history</h3>
            </div>
          </div>

          <label className="inline-select">
            <span>Profile</span>
            <select
              value={selectedProfileId}
              onChange={(event) => {
                setSelectedProfileId(event.target.value);
                setResolvedPlan(null);
              }}
            >
              {profiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.name}
                </option>
              ))}
            </select>
          </label>

          <div className="panel-actions">
            <button
              onClick={() =>
                selectedProfileId && resolvePlanMutation.mutate(selectedProfileId)
              }
              disabled={!selectedProfileId}
            >
              {resolvePlanMutation.isPending ? "Loading..." : "Preview launch"}
            </button>
            <button
              className="primary"
              onClick={() => selectedProfileId && launchMutation.mutate(selectedProfileId)}
              disabled={!selectedProfileId}
            >
              {launchMutation.isPending ? "Launching..." : "Launch profile"}
            </button>
          </div>

          {launchTarget ? (
            <p className="muted">
              Account: {launchTarget.accountId ?? "None selected"}
            </p>
          ) : null}
          <p className="muted">
            Blocksmith prefers a version-matched managed Java runtime. A
            profile-specific Java path overrides it when compatible.
          </p>

          {resolvedPlan ? (
            <>
              <div className="detail-grid">
                <div>
                  <strong>User</strong>
                  <p>{resolvedPlan.username}</p>
                </div>
                <div>
                  <strong>Session</strong>
                  <p>{resolvedPlan.online ? "online" : "offline"}</p>
                </div>
                <div>
                  <strong>Main class</strong>
                  <p className="mono">{resolvedPlan.mainClass}</p>
                </div>
                <div>
                  <strong>Java</strong>
                  <p className="mono">{resolvedPlan.javaExecutable}</p>
                </div>
              </div>

              <p className="mono">
                {resolvedPlan.commandPreview.slice(0, 8).join(" ")}
              </p>
            </>
          ) : (
            <EmptyState
              eyebrow="Launch"
              title="Preview launch before you start"
              body="Review the selected account, Java runtime, and command details for this profile."
            />
          )}

          {(launchHistoryQuery.data ?? []).length > 0 ? (
            <div className="card-list compact">
              {(launchHistoryQuery.data ?? []).map((entry) => (
                <article className="result-card" key={entry.id}>
                  <div className="result-card-header">
                    <div>
                      <h4>{entry.status}</h4>
                      <p className="muted">{formatDateTime(entry.startedAt)}</p>
                    </div>
                    <span className="mono">
                      {entry.exitCode === null ? "running" : entry.exitCode}
                    </span>
                  </div>
                  <p className="mono">{entry.logPath}</p>
                </article>
              ))}
            </div>
          ) : null}

          {resolvePlanMutation.error ? (
            <p className="error-text">{String(resolvePlanMutation.error)}</p>
          ) : null}
          {launchMutation.error ? (
            <p className="error-text">{String(launchMutation.error)}</p>
          ) : null}
          {launchHistoryQuery.error ? (
            <p className="error-text">{String(launchHistoryQuery.error)}</p>
          ) : null}
        </section>

        <section className="panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Installed content</p>
              <h3>Toggle and remove content</h3>
            </div>
          </div>

          <label className="inline-select">
            <span>Profile</span>
            <select
              value={selectedProfileId}
              onChange={(event) => setSelectedProfileId(event.target.value)}
            >
              {profiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.name}
                </option>
              ))}
            </select>
          </label>

          {(installedContentQuery.data ?? []).length > 0 ? (
            <div className="card-list compact">
              {(installedContentQuery.data ?? []).map((item) => (
                <article className="result-card" key={item.id}>
                  <div className="result-card-header">
                    <div>
                      <h4>{item.name}</h4>
                      <p className="muted">
                        {item.contentType} / {item.versionNumber ?? item.versionId}
                      </p>
                    </div>
                    <div className="row-actions">
                      {item.contentType === "mod" ? (
                        <button
                          onClick={() =>
                            toggleMutation.mutate({
                              installedContentId: item.id,
                              enabled: !item.enabled,
                            })
                          }
                        >
                          {item.enabled ? "Disable" : "Enable"}
                        </button>
                      ) : null}
                      <button
                        className="danger-ghost"
                        onClick={() => removeContentMutation.mutate(item.id)}
                      >
                        Remove
                      </button>
                    </div>
                  </div>
                  <p className="mono">{item.localFilePath}</p>
                  <p className="muted">
                    Installed {formatDateTime(item.installedAt)}
                  </p>
                </article>
              ))}
            </div>
          ) : (
            <EmptyState
              eyebrow="Content"
              title="No installed content for this profile"
              body="Use Discover to search Modrinth, import a .mrpack file, or install content for this profile."
            />
          )}

          {toggleMutation.error ? (
            <p className="error-text">{String(toggleMutation.error)}</p>
          ) : null}
          {removeContentMutation.error ? (
            <p className="error-text">{String(removeContentMutation.error)}</p>
          ) : null}
        </section>
      </div>

      <section className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Share</p>
            <h3>Export and import profiles</h3>
          </div>
        </div>

        <div className="toolbar-grid">
          <label>
            <span>Export profile</span>
            <select
              value={selectedProfileId}
              onChange={(event) => setSelectedProfileId(event.target.value)}
            >
              {profiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.name}
                </option>
              ))}
            </select>
          </label>

          <label>
            <span>Import name override</span>
            <input
              value={shareImportName}
              onChange={(event) => setShareImportName(event.target.value)}
              placeholder="Optional imported profile name"
            />
          </label>

          <label>
            <span>Import manifest file</span>
            <input
              value={shareFilePath}
              onChange={(event) => setShareFilePath(event.target.value)}
              placeholder="C:\\Path\\To\\profile.blocksmith.json"
            />
          </label>

          <label className="span-2">
            <span>Share code</span>
            <textarea
              rows={5}
              value={shareCode}
              onChange={(event) => setShareCode(event.target.value)}
              placeholder="Export a profile to generate a share code, or paste one here to import."
            />
          </label>
        </div>

        <div className="panel-actions">
          <button
            onClick={() => exportMutation.mutate(selectedProfileId)}
            disabled={!selectedProfileId}
          >
            {exportMutation.isPending ? "Exporting..." : "Export code"}
          </button>
          <button
            onClick={() => importFileMutation.mutate()}
            disabled={!shareFilePath.trim()}
          >
            {importFileMutation.isPending ? "Importing..." : "Import file"}
          </button>
          <button
            className="primary"
            onClick={() => importMutation.mutate()}
            disabled={!shareCode.trim()}
          >
            {importMutation.isPending ? "Importing..." : "Import profile"}
          </button>
          {lastExportPath ? <span className="mono">{lastExportPath}</span> : null}
        </div>

        {exportMutation.error ? (
          <p className="error-text">{String(exportMutation.error)}</p>
        ) : null}
        {importMutation.error ? (
          <p className="error-text">{String(importMutation.error)}</p>
        ) : null}
        {importFileMutation.error ? (
          <p className="error-text">{String(importFileMutation.error)}</p>
        ) : null}
      </section>
    </div>
  );
}
